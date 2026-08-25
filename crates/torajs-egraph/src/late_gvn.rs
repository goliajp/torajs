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
//! Misses are always safe (a missed dedup costs a recomputation,
//! never correctness). Hits replace the second instruction's result
//! with the first everywhere (def dominates both, so every rewritten
//! use stays dominated) and delete the redundant instruction — the
//! deleted kinds are `classify_pure`, so no memory or refcount effect
//! disappears with them.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, InstKind, Module, Operand, Terminator, Type, ValueId,
    rewrite_value_operands, visit_value_operands,
};

use crate::dominator::DominatorTree;
use crate::optimize::classify_pure;

#[derive(Debug, Default)]
pub struct LateGvnStats {
    /// Redundant pure instructions deleted (uses rewritten to the
    /// dominating twin).
    pub dedups: u32,
}

type Key = (Type, InstKind, Vec<u32>);

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

/// Values defined by more than one instruction — the φ-web Copies
/// MEM2REG_PHI leaves behind. Everything else is single-def SSA.
fn multi_def_values(func: &Function) -> HashSet<ValueId> {
    let mut seen: HashSet<ValueId> = HashSet::new();
    let mut multi: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result
                && !seen.insert(r)
            {
                multi.insert(r);
            }
        }
    }
    multi
}

/// One dominator-walk scope frame. `barrier` marks a block whose
/// predecessor set is not exactly its immediate dominator — risky
/// entries above it are invisible below (see module doc).
/// Scope-exit removal is by exact key: an inner insert that shadows
/// an outer entry drops the outer one at inner exit — a conservative
/// loss (later lookups miss and recompute), never a stale hit.
struct Frame {
    safe: Vec<Key>,
    risky: Vec<Key>,
    barrier: bool,
}

struct Walk {
    safe: HashMap<Key, ValueId>,
    risky: HashMap<Key, (ValueId, usize)>,
    frames: Vec<Frame>,
    barrier_depth: Vec<usize>,
    vgen: HashMap<ValueId, u32>,
    replace: HashMap<ValueId, ValueId>,
    multi: HashSet<ValueId>,
}

impl Walk {
    fn resolve(&self, mut v: ValueId) -> ValueId {
        while let Some(&n) = self.replace.get(&v) {
            v = n;
        }
        v
    }

    fn key_for(&self, ty: Type, kind: &InstKind) -> (Key, bool) {
        let mut resolved = kind.clone();
        rewrite_value_operands(&mut resolved, |v| self.resolve(v));
        let mut gens: Vec<u32> = Vec::new();
        let mut risky = false;
        visit_value_operands(&resolved, |v| {
            if self.multi.contains(&v) {
                risky = true;
                gens.push(self.vgen.get(&v).copied().unwrap_or(0));
            } else {
                gens.push(0);
            }
        });
        ((ty, resolved, gens), risky)
    }

    fn enter(&mut self, barrier: bool) {
        self.frames.push(Frame {
            safe: Vec::new(),
            risky: Vec::new(),
            barrier,
        });
        if barrier {
            self.barrier_depth.push(self.frames.len());
        }
    }

    fn exit(&mut self) {
        let f = self.frames.pop().expect("scope underflow");
        for k in f.safe {
            self.safe.remove(&k);
        }
        for k in f.risky {
            self.risky.remove(&k);
        }
        if f.barrier {
            self.barrier_depth.pop();
        }
    }

    /// A risky entry is visible only when it was inserted at or below
    /// the innermost barrier (i.e. along the unique-pred chain).
    fn risky_visible(&self, depth: usize) -> bool {
        self.barrier_depth.last().is_none_or(|&b| depth >= b)
    }
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
    let mut w = Walk {
        safe: HashMap::new(),
        risky: HashMap::new(),
        frames: Vec::new(),
        barrier_depth: Vec::new(),
        vgen: HashMap::new(),
        replace: HashMap::new(),
        multi: multi_def_values(func),
    };

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
                stack.push(Visit::Exit);
                for &child in dom.children(bid) {
                    stack.push(Visit::Enter(child));
                }
                process_block(func, bi, &mut w, stats);
            }
        }
    }

    // Apply: rewrite every use through the replace chain and drop the
    // deduplicated instructions.
    if w.replace.is_empty() {
        return;
    }
    let resolve_full = |v: ValueId, w: &Walk| w.resolve(v);
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
}

fn process_block(func: &Function, bi: usize, w: &mut Walk, stats: &mut LateGvnStats) {
    for inst in &func.blocks[bi].insts {
        let Some(result) = inst.result else { continue };
        if w.multi.contains(&result) {
            // A φ-web redefinition — later lookups keyed on this
            // value must miss from here on.
            *w.vgen.entry(result).or_insert(0) += 1;
            continue;
        }
        if !classify_pure(&inst.kind) {
            continue;
        }
        let (key, risky) = w.key_for(func.values[result.0 as usize].ty, &inst.kind);
        if risky {
            match w.risky.get(&key) {
                Some(&(existing, depth)) if w.risky_visible(depth) => {
                    w.replace.insert(result, existing);
                    stats.dedups += 1;
                }
                _ => {
                    let depth = w.frames.len();
                    w.risky.insert(key.clone(), (result, depth));
                    w.frames.last_mut().expect("in scope").risky.push(key);
                }
            }
        } else if let Some(&existing) = w.safe.get(&key) {
            w.replace.insert(result, existing);
            stats.dedups += 1;
        } else {
            w.safe.insert(key.clone(), result);
            w.frames.last_mut().expect("in scope").safe.push(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, Inst, ValueInfo};

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
}
