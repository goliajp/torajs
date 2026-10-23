//! Post-φ dominator GVN — mandelbrot decomposition D2.
//!
//! The main egraph GVN runs before φ promotion (deliberately: it
//! must not see `MEM2REG_PHI`'s multi-def Copies), which means every
//! loop-carried variable is still a slot Load at GVN time — two
//! textually identical `zr * zr` reads are two different Load results
//! and never deduplicate. This pass re-runs a scoped GVN after φ
//! promotion, when those operands are registers.
//!
//! Multi-def values (the φ web's Copy targets) break the SSA "same
//! operands ⇒ same value" premise, so entries whose key touches one
//! ride a *restricted* scope:
//!
//! - **safe table** — every value operand is single-def SSA. Valid
//!   across the whole dominator subtree (classic dominator GVN).
//! - **risky table** — some operand is multi-def. An entry is only
//!   visible along a chain of blocks each of which has its dominator
//!   as its *unique* CFG predecessor (an extended-basic-block chain:
//!   no join can smuggle in a path that redefined the operand without
//!   also re-executing the entry's defining inst), and within-block
//!   redefinitions bump a per-value generation stamped into the key,
//!   so later lookups in the same chain miss after a redef.
//!
//! A load, at a constant or a computed offset, is keyed by the memory
//! epoch it runs in as well (`epochs.rs`): two loads of one address
//! fold only when no path between them can have stored. One whose
//! operands are single-def rides the safe table under that key; one
//! that reads a multi-def value rides the risky table. A store enters
//! its value as the load of its own address in the epoch it opens.
//!
//! Misses are always safe (a missed dedup costs a recomputation,
//! never correctness). Hits replace the second instruction's result
//! with the first everywhere (def dominates both, so every rewritten
//! use stays dominated) and delete the redundant instruction — the
//! deleted kinds are `classify_pure` or a load, so no memory or
//! refcount effect disappears with them. A repeated address cast keys
//! as its dominating twin and stays in place; once the dedup has taken
//! its last reader it is deleted too (`keys::drop_unread_casts`), a
//! cast between an address and its bits having no effect either.

mod epochs;
mod keys;
mod known_branch;
mod view_guard;
mod walk;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, Module, Operand, Terminator, ValueId, rewrite_value_operands,
    visit_value_operands,
};

use crate::float_demote::cfg::multi_def_values;
use crate::optimize::classify_pure;
use epochs::may_store;
use keys::{global_ref_twins, is_address_cast, is_read};
use torajs_core::ssa::dominator::DominatorTree;
use walk::Walk;

#[derive(Debug, Default)]
pub struct LateGvnStats {
    /// Redundant pure instructions deleted (uses rewritten to the
    /// dominating twin).
    pub dedups: u32,
}

/// The dedup, the receiver guards of inline views their join's
/// predecessors decide, then the conditionals left testing a value a
/// branch above them already tested.
pub(crate) fn run(module: &mut Module) {
    crate::gated_pass("LATE_GVN", module, dedup_pure_ops);
    crate::gated_pass("VIEW_GUARD", module, view_guard::thread_view_guards);
    crate::gated_pass("KNOWN_BRANCH", module, known_branch::fold_known_branches);
}

pub fn dedup_pure_ops(module: &mut Module) -> LateGvnStats {
    let mut stats = LateGvnStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        gvn_fn(func, &mut stats);
    }
    stats
}

fn gvn_fn(func: &mut Function, stats: &mut LateGvnStats) {
    let dom = DominatorTree::compute(func);
    let n = func.blocks.len();
    let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for block in &func.blocks {
        let succs: Vec<BlockId> = match &block.term {
            Terminator::Br(t) => vec![*t],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => vec![*then_blk, *else_blk],
            Terminator::Ret(_) | Terminator::Unreachable => vec![],
        };
        for s in succs {
            if (s.0 as usize) < n && !preds[s.0 as usize].contains(&block.id) {
                preds[s.0 as usize].push(block.id);
            }
        }
    }
    let multi = multi_def_values(func);
    let mut w = Walk {
        safe: HashMap::default(),
        risky: HashMap::default(),
        frames: Vec::new(),
        barrier_depth: Vec::new(),
        vgen: HashMap::default(),
        replace: HashMap::default(),
        twin: global_ref_twins(func, &multi),
        cast_twins: HashSet::default(),
        multi,
        epoch: 0,
    };
    let epochs = epochs::number(func, &preds);

    enum Visit {
        Enter(BlockId),
        Exit,
    }
    let mut stack = vec![Visit::Enter(BlockId(0))];
    while let Some(v) = stack.pop() {
        match v {
            Visit::Exit => w.exit(),
            Visit::Enter(bid) => {
                let bi = bid.0 as usize;
                if bi >= n {
                    continue;
                }
                let barrier =
                    preds[bi].len() != 1 || dom.immediate_dominator(bid) != Some(preds[bi][0]);
                w.enter(barrier && bid.0 != 0);
                w.epoch = epochs.entry[bi];
                stack.push(Visit::Exit);
                for &child in dom.children(bid) {
                    stack.push(Visit::Enter(child));
                }
                process_block(func, bi, &epochs.minted[bi], &mut w, stats);
            }
        }
    }

    // Apply: rewrite every use through the replace chain and drop the
    // deduplicated instructions.
    if w.replace.is_empty() {
        return;
    }
    let resolve_full = |v: ValueId, w: &Walk| w.resolve(v);
    // a repeated cast loses its last reader only with a deleted one
    let freed = !w.cast_twins.is_empty() && deletes_a_cast_reader(func, &w);
    for block in func.blocks.iter_mut() {
        block
            .insts
            .retain(|inst| !inst.result.is_some_and(|r| w.replace.contains_key(&r)));
        for inst in block.insts.iter_mut() {
            rewrite_value_operands(&mut inst.kind, |v| resolve_full(v, &w));
        }
        let fix = |op: &mut Operand| {
            if let Operand::Value(v) = op {
                *v = resolve_full(*v, &w);
            }
        };
        match &mut block.term {
            Terminator::CondBr { cond, .. } => fix(cond),
            Terminator::Ret(Some(op)) => fix(op),
            _ => {}
        }
    }
    if freed {
        keys::drop_unread_casts(func, &w.cast_twins);
    }
}

fn deletes_a_cast_reader(func: &Function, w: &Walk) -> bool {
    let mut reads = false;
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if inst.result.is_some_and(|r| w.replace.contains_key(&r)) {
            visit_value_operands(&inst.kind, |v| reads |= w.cast_twins.contains(&v));
        }
    }
    reads
}

fn process_block(
    func: &Function,
    bi: usize,
    minted: &[u32],
    w: &mut Walk,
    stats: &mut LateGvnStats,
) {
    let mut minted = minted.iter();
    for (at, inst) in func.blocks[bi].insts.iter().enumerate() {
        let load = is_read(&inst.kind);
        let pure = classify_pure(&inst.kind);
        if may_store(&inst.kind) {
            w.epoch = *minted.next().expect("one number per storing instruction");
            w.note_store(func, bi, at);
        }
        let Some(result) = inst.result else { continue };
        if w.multi.contains(&result) {
            // A φ-web redefinition — later lookups keyed on this
            // value must miss from here on.
            *w.vgen.entry(result).or_insert(0) += 1;
            continue;
        }
        let ty = func.values[result.0 as usize].ty;
        let cast = !pure && !load && is_address_cast(func, &inst.kind, ty);
        if !pure && !load && !cast {
            continue;
        }
        let (mut key, risky) = w.key_for(ty, &inst.kind);
        if load {
            key.2.push(w.epoch);
        }
        if risky {
            match w.risky.get(&key) {
                Some(&(existing, depth)) if w.risky_visible(depth) => {
                    w.hit(cast, result, existing, stats);
                }
                _ => {
                    let depth = w.frames.len();
                    w.risky.insert(key.clone(), (result, depth));
                    w.frames.last_mut().expect("in scope").risky.push(key);
                }
            }
        } else if let Some(&existing) = w.safe.get(&key) {
            w.hit(cast, result, existing, stats);
        } else {
            w.safe.insert(key.clone(), result);
            w.frames.last_mut().expect("in scope").safe.push(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, Inst, InstKind, Type, ValueInfo};

    fn vi(ty: Type) -> ValueInfo {
        ValueInfo { ty, name: None }
    }
    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }
    fn func_of(values: Vec<ValueInfo>, blocks: Vec<Block>) -> Module {
        Module {
            funcs: vec![Function {
                name: "f".into(),
                params: vec![],
                ret: Type::I64,
                values,
                blocks,
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    /// The mandelbrot shape: %zr (φ multi-def) squared in the loop
    /// header, squared again in the body (unique-pred child of the
    /// header) — the second multiply dedups onto the first.
    #[test]
    fn header_to_body_ebb_chain_dedups() {
        // v0 = zr (multi-def: entry Copy + latch Copy), v1 cond,
        // v2 = zr*zr (header), v3 = zr*zr (body), v4 latch redef temp.
        let mut m = func_of(
            vec![
                vi(Type::F64),
                vi(Type::Bool),
                vi(Type::F64),
                vi(Type::F64),
                vi(Type::F64),
            ],
            vec![
                Block {
                    id: BlockId(0),
                    insts: vec![mk(0, InstKind::Copy(Type::F64, Operand::ConstF64(0.0)))],
                    term: Terminator::Br(BlockId(1)),
                },
                // header: preds {entry, body} — barrier, but its own
                // entries flow to the body below.
                Block {
                    id: BlockId(1),
                    insts: vec![
                        mk(
                            2,
                            InstKind::BinOp(
                                BinOp::FMul,
                                Operand::Value(ValueId(0)),
                                Operand::Value(ValueId(0)),
                            ),
                        ),
                        mk(
                            1,
                            InstKind::FCmp(
                                torajs_core::ssa::FPred::Ogt,
                                Operand::Value(ValueId(2)),
                                Operand::ConstF64(4.0),
                            ),
                        ),
                    ],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(3),
                        else_blk: BlockId(2),
                    },
                },
                // body: unique pred = header = idom — EBB chain.
                Block {
                    id: BlockId(2),
                    insts: vec![
                        mk(
                            3,
                            InstKind::BinOp(
                                BinOp::FMul,
                                Operand::Value(ValueId(0)),
                                Operand::Value(ValueId(0)),
                            ),
                        ),
                        // latch redef of zr AFTER the reuse.
                        mk(0, InstKind::Copy(Type::F64, Operand::Value(ValueId(3)))),
                    ],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(3),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
            ],
        );
        let stats = dedup_pure_ops(&mut m);
        assert_eq!(stats.dedups, 1);
        let body = &m.funcs[0].blocks[2].insts;
        // The duplicate fmul is gone; the latch Copy now reads v2.
        assert_eq!(body.len(), 1);
        assert!(matches!(
            body[0].kind,
            InstKind::Copy(Type::F64, Operand::Value(v)) if v == ValueId(2)
        ));
    }

    /// A join block (two preds) must NOT inherit risky entries: the
    /// other path may have redefined the multi-def operand without
    /// re-executing the defining inst.
    #[test]
    fn join_blocks_do_not_inherit_risky_entries() {
        // v0 multi-def; diamond: b0 defines v1 = v0*v0, branches to
        // b1 (redefines v0) / b2 (empty), both -> b3 which computes
        // v0*v0 again — must NOT dedup onto v1.
        let mut m = func_of(
            vec![vi(Type::F64), vi(Type::F64), vi(Type::Bool), vi(Type::F64)],
            vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        mk(0, InstKind::Copy(Type::F64, Operand::ConstF64(1.0))),
                        mk(
                            1,
                            InstKind::BinOp(
                                BinOp::FMul,
                                Operand::Value(ValueId(0)),
                                Operand::Value(ValueId(0)),
                            ),
                        ),
                        mk(
                            2,
                            InstKind::FCmp(
                                torajs_core::ssa::FPred::Ogt,
                                Operand::Value(ValueId(1)),
                                Operand::ConstF64(0.5),
                            ),
                        ),
                    ],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(2)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![mk(0, InstKind::Copy(Type::F64, Operand::ConstF64(2.0)))],
                    term: Terminator::Br(BlockId(3)),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Br(BlockId(3)),
                },
                Block {
                    id: BlockId(3),
                    insts: vec![mk(
                        3,
                        InstKind::BinOp(
                            BinOp::FMul,
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(0)),
                        ),
                    )],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                },
            ],
        );
        let stats = dedup_pure_ops(&mut m);
        assert_eq!(stats.dedups, 0, "join must block the risky entry");
        assert_eq!(m.funcs[0].blocks[3].insts.len(), 1);
    }

    /// Single-def operands dedup across joins freely (safe table).
    #[test]
    fn safe_entries_survive_joins() {
        // v0 single-def; b0: v1 = v0+v0 → diamond → b3: v3 = v0+v0.
        let mut m = func_of(
            vec![vi(Type::I64), vi(Type::I64), vi(Type::Bool), vi(Type::I64)],
            vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        mk(0, InstKind::Copy(Type::I64, Operand::ConstI64(7))),
                        mk(
                            1,
                            InstKind::BinOp(
                                BinOp::Add,
                                Operand::Value(ValueId(0)),
                                Operand::Value(ValueId(0)),
                            ),
                        ),
                        mk(
                            2,
                            InstKind::ICmp(
                                torajs_core::ssa::IPred::Sgt,
                                Operand::Value(ValueId(1)),
                                Operand::ConstI64(3),
                            ),
                        ),
                    ],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(2)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Br(BlockId(3)),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Br(BlockId(3)),
                },
                Block {
                    id: BlockId(3),
                    insts: vec![mk(
                        3,
                        InstKind::BinOp(
                            BinOp::Add,
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(0)),
                        ),
                    )],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                },
            ],
        );
        let stats = dedup_pure_ops(&mut m);
        assert_eq!(stats.dedups, 1);
        assert!(m.funcs[0].blocks[3].insts.is_empty());
        assert!(matches!(
            m.funcs[0].blocks[3].term,
            Terminator::Ret(Some(Operand::Value(v))) if v == ValueId(1)
        ));
    }

    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }
    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }
    fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }
    fn field(result: u32) -> Inst {
        mk(result, InstKind::Load(Type::I64, v(0), 8))
    }

    /// b0: %1 = load %0+8, then `between`, then `again`, and
    /// b0 → (b1: `arm` | b2) → b3: %4 = load %0+8; ret %4.
    fn reads(between: Vec<Inst>, again: Vec<Inst>, arm: Vec<Inst>) -> Module {
        let mut head = vec![field(1)];
        head.extend(between);
        head.extend(again);
        func_of(
            vec![
                vi(Type::Ptr),
                vi(Type::I64),
                vi(Type::I64),
                vi(Type::I64),
                vi(Type::I64),
                vi(Type::Ptr),
            ],
            vec![
                blk(
                    0,
                    head,
                    Terminator::CondBr {
                        cond: Operand::ConstBool(true),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(1, arm, Terminator::Br(BlockId(3))),
                blk(2, vec![], Terminator::Br(BlockId(3))),
                blk(3, vec![field(4)], Terminator::Ret(Some(v(4)))),
            ],
        )
    }
    fn returned(m: &Module) -> ValueId {
        match m.funcs[0].blocks[3].term {
            Terminator::Ret(Some(Operand::Value(r))) => r,
            _ => panic!("ret of a value"),
        }
    }

    #[test]
    fn a_load_repeated_past_a_join_that_stored_nothing_dedups() {
        let mut m = reads(vec![], vec![], vec![]);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 1);
        assert_eq!(returned(&m), ValueId(1));
    }

    #[test]
    fn a_load_past_an_arm_that_only_casts_an_address_dedups() {
        let arm = vec![
            mk(2, InstKind::PtrToInt(v(0))),
            mk(5, InstKind::IntToPtr(v(2))),
        ];
        let mut m = reads(vec![], vec![], arm);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 1);
        assert_eq!(returned(&m), ValueId(1));
    }

    /// b0: %2 = ptrtoint %0; %5 = inttoptr %2; %1 = load %5+8;
    /// %3 = ptrtoint %0; %6 = inttoptr %3; %4 = load %6+8; ret %4.
    #[test]
    fn a_load_through_a_cast_repeated_reads_the_first_load() {
        let ptr = |r, o| mk(r, InstKind::IntToPtr(v(o)));
        let at = |r, p| mk(r, InstKind::Load(Type::I64, v(p), 8));
        let mut values = vec![vi(Type::Ptr); 7];
        for i in [1, 2, 3, 4] {
            values[i] = vi(Type::I64);
        }
        let insts = vec![
            mk(2, InstKind::PtrToInt(v(0))),
            ptr(5, 2),
            at(1, 5),
            mk(3, InstKind::PtrToInt(v(0))),
            ptr(6, 3),
            at(4, 6),
        ];
        let mut m = func_of(values, vec![blk(0, insts, Terminator::Ret(Some(v(4))))]);
        // the second load goes, and the casts nothing reads any more
        // with it
        assert_eq!(dedup_pure_ops(&mut m).dedups, 1);
        assert_eq!(m.funcs[0].blocks[0].insts.len(), 3);
        let Terminator::Ret(Some(Operand::Value(r))) = m.funcs[0].blocks[0].term else {
            panic!("ret of a value");
        };
        assert_eq!(r, ValueId(1));
    }

    #[test]
    fn a_load_past_an_arm_that_stores_stays() {
        let arm = vec![void(InstKind::Store(Operand::ConstI64(7), v(0), 8))];
        let mut m = reads(vec![], vec![], arm);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 0);
        assert_eq!(returned(&m), ValueId(4));
    }

    #[test]
    fn a_computed_offset_load_and_an_address_constant_keep_the_epoch() {
        let between = vec![
            mk(5, InstKind::GlobalRef("g".into())),
            mk(2, InstKind::LoadDyn(Type::I64, v(0), v(1))),
        ];
        let mut m = reads(between, vec![field(3)], vec![]);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 2);
        assert_eq!(returned(&m), ValueId(1));
    }

    /// b0 → (b1: %1 = copy 1 | b2: %1 = copy 2) → b3: store %1, %0+8;
    /// %4 = load %0+8; `tail`; ret %4.
    fn merged_then_stored(tail: Vec<Inst>) -> Module {
        let cell = |k: i64| vec![mk(1, InstKind::Copy(Type::I64, Operand::ConstI64(k)))];
        let mut last = vec![void(InstKind::Store(v(1), v(0), 8)), field(4)];
        last.extend(tail);
        func_of(
            vec![
                vi(Type::Ptr),
                vi(Type::I64),
                vi(Type::I64),
                vi(Type::I64),
                vi(Type::I64),
            ],
            vec![
                blk(
                    0,
                    vec![],
                    Terminator::CondBr {
                        cond: Operand::ConstBool(true),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(1, cell(1), Terminator::Br(BlockId(3))),
                blk(2, cell(2), Terminator::Br(BlockId(3))),
                blk(3, last, Terminator::Ret(Some(v(4)))),
            ],
        )
    }

    #[test]
    fn a_load_after_a_store_of_a_merged_cell_reads_the_cell() {
        let mut m = merged_then_stored(vec![]);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 1);
        assert_eq!(returned(&m), ValueId(1));
        // the cell is written again after the store: the load keeps
        // what memory held
        let again = mk(1, InstKind::Copy(Type::I64, Operand::ConstI64(3)));
        let mut m = merged_then_stored(vec![again]);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 0);
        assert_eq!(returned(&m), ValueId(4));
    }

    #[test]
    fn a_load_after_a_store_to_its_address_reads_the_stored_value() {
        let between = vec![void(InstKind::Store(v(1), v(0), 8))];
        let mut m = reads(between, vec![field(3)], vec![]);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 2);
        assert_eq!(returned(&m), ValueId(1));
        // another offset, or a value of another type, is not that load
        let between = vec![void(InstKind::Store(v(1), v(0), 16))];
        let mut m = reads(between, vec![field(3)], vec![]);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 1);
        assert_eq!(returned(&m), ValueId(3));
        let between = vec![void(InstKind::Store(v(5), v(0), 8))];
        let mut m = reads(between, vec![field(3)], vec![]);
        assert_eq!(dedup_pure_ops(&mut m).dedups, 1);
        assert_eq!(returned(&m), ValueId(3));
    }
}
