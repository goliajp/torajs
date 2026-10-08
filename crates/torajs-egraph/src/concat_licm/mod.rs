//! Loop-invariant string concatenation — `prefix + tail` with both
//! operands unchanged across the loop is built once, in a preheader,
//! and every iteration takes a reference to that one cell instead of
//! building, copying and freeing its own.
//!
//! ```text
//! G:  cond_br %c, H, X          G:  cond_br %c, P, X
//! H:  %s = call concat(%a, %b)  P:  %h = call concat(%a, %b)
//!     ...uses of %s...              br H
//!     cond_br %c', H, X         H:  rc_inc %h
//!                                   ...uses of %h...
//!                                   cond_br %c', H, E
//!                               E:  call str_drop(%h)
//!                                   br X
//! ```
//!
//! A concat is pure in its operands (ECMA-262 §6.1.4: a String is
//! its sequence of code units, and the cells here are immutable once
//! built; a kernel that grows its left operand in place takes the
//! caller's reference and is not in this family).
//!
//! What the kernel still does is allocate and, past the length bound,
//! raise a RangeError, so it may not run on a path the program would
//! not have taken: the concat must sit in the loop header — the block
//! every entry into the loop executes — behind nothing but
//! effect-free instructions, and the preheader is a new block on the
//! loop's single entry edge. A top-tested `while` keeps its exit test
//! in the header and its body in the next block, so this applies to
//! the loops `loop_rotate` has turned bottom-tested, and runs after it.
//! A loop that catches the RangeError and runs on keeps its concats
//! (`catch`).
//!
//! Ownership: the kernel answers a fresh cell at +1 which the body's
//! lowering consumes exactly once on every path (a slot store takes
//! it, a block close or a throw exit drops it). The hoisted cell's
//! own reference belongs to the loop: the header mints the body's +1
//! with an `rc_inc`, so every later use keeps the accounting it was
//! lowered with, and every edge out of the loop releases the loop's
//! reference.
//!
//! `TORAJS_CONCAT_LICM_OFF=1` skips, `TORAJS_CONCAT_LICM_STATS=1`
//! counts.

mod catch;
mod sink;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, FuncId, Function, Inst, InstKind, Module, Operand, Terminator, ValueId,
    rewrite_value_operands,
};

use crate::cfg_edit::new_block;
use crate::float_demote::cfg::successors;
use crate::float_demote::push_value;
use crate::optimize::is_pure;
use crate::str_append::kernels::Kernels;
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConcatLicmStats {
    /// Concat calls moved out of a loop.
    pub concats_hoisted: u32,
}

pub fn hoist_concats(module: &mut Module) -> ConcatLicmStats {
    let mut stats = ConcatLicmStats::default();
    let Some(kernels) = Kernels::find(module) else {
        return stats;
    };
    let borrowing: Vec<FuncId> = kernels.borrowing().collect();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        // one loop per analysis: the preheader and exit blocks a hoist
        // adds change the loop nest the next one is judged on
        while let Some(plan) = find_hoist(func, &borrowing) {
            stats.concats_hoisted += plan.calls.len() as u32;
            apply(func, plan, kernels.str_drop);
        }
    }
    stats
}

struct Hoist {
    header: BlockId,
    /// The one block outside the loop that branches to the header.
    entry: BlockId,
    body: HashSet<BlockId>,
    /// Positions in the header of the calls to move, ascending.
    calls: Vec<usize>,
}

/// The first loop, innermost first, whose header opens with a
/// hoistable concat.
fn find_hoist(func: &Function, borrowing: &[FuncId]) -> Option<Hoist> {
    let dom = DominatorTree::compute(func);
    let mut loops = LoopAnalysis::compute(func, &dom).loops().to_vec();
    loops.sort_by_key(|l| l.body.len());
    let mut defs: HashMap<ValueId, Vec<BlockId>> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push(block.id);
            }
        }
    }
    for lp in loops {
        let body: HashSet<BlockId> = lp.body.iter().copied().collect();
        let entries: Vec<BlockId> = func
            .blocks
            .iter()
            .filter(|b| !body.contains(&b.id) && successors(&b.term).any(|s| s == lp.header))
            .map(|b| b.id)
            .collect();
        let [entry] = entries.as_slice() else {
            continue;
        };
        // an earlier hoisted concat's result is built in the preheader
        // too, so a later one may read it
        let mut hoisted: HashSet<ValueId> = HashSet::default();
        let invariant = |op: &Operand, hoisted: &HashSet<ValueId>| match op {
            Operand::Value(v) => {
                hoisted.contains(v)
                    || defs
                        .get(v)
                        .is_none_or(|ds| ds.iter().all(|d| !body.contains(d)))
            }
            _ => true,
        };
        let mut calls = Vec::new();
        for (i, inst) in func.blocks[lp.header.0 as usize].insts.iter().enumerate() {
            match (&inst.kind, inst.result) {
                // the result is renamed to the hoisted cell everywhere,
                // so the call must be its only definition
                (InstKind::Call(fid, args), Some(r))
                    if defs[&r].len() == 1
                        && borrowing.contains(fid)
                        && args.iter().all(|a| invariant(a, &hoisted)) =>
                {
                    calls.push(i);
                    hoisted.insert(r);
                }
                (kind, _) if effect_free(kind) => {}
                _ => break,
            }
        }
        if !calls.is_empty() && !catch::catches_inside(func, &body) {
            return Some(Hoist {
                header: lp.header,
                entry: *entry,
                body,
                calls,
            });
        }
    }
    None
}

/// Nothing the program can observe: no store, no call, no release.
fn effect_free(kind: &InstKind) -> bool {
    is_pure(kind)
        || matches!(
            kind,
            InstKind::Load(..)
                | InstKind::LoadDyn(..)
                | InstKind::LoadDynScaled8(..)
                | InstKind::LoadU8Dyn(..)
                | InstKind::LoadU16Dyn(..)
                | InstKind::GlobalRef(_)
                | InstKind::StaticStrRef(_)
                | InstKind::Copy(..)
                | InstKind::RcInc(_)
                | InstKind::RcIncAny(_)
        )
}

fn apply(func: &mut Function, mut plan: Hoist, str_drop: FuncId) {
    let pre = new_block(func);
    retarget(
        &mut func.blocks[plan.entry.0 as usize].term,
        plan.header,
        pre,
    );
    func.blocks[pre.0 as usize].term = Terminator::Br(plan.header);
    let mut rename: HashMap<ValueId, ValueId> = HashMap::default();
    let mut hoisted = Vec::with_capacity(plan.calls.len());
    for &i in &plan.calls {
        let r = func.blocks[plan.header.0 as usize].insts[i]
            .result
            .take()
            .expect("a hoisted call defines its result");
        let ty = func.values[r.0 as usize].ty;
        let h = push_value(&mut func.values, ty);
        let inst = &mut func.blocks[plan.header.0 as usize].insts[i];
        let kind = std::mem::replace(&mut inst.kind, InstKind::RcInc(Operand::Value(h)));
        rename.insert(r, h);
        hoisted.push(Inst {
            result: Some(h),
            kind,
            origin: inst.origin,
        });
    }
    // a later hoisted concat reads an earlier one's cell
    for inst in hoisted.iter_mut() {
        rewrite_value_operands(&mut inst.kind, |v| rename.get(&v).copied().unwrap_or(v));
    }
    func.blocks[pre.0 as usize].insts = hoisted;
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_value_operands(&mut inst.kind, |v| rename.get(&v).copied().unwrap_or(v));
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &mut block.term
            && let Some(h) = rename.get(v)
        {
            *v = *h;
        }
    }
    let mut cells: Vec<ValueId> = rename.values().copied().collect();
    cells.sort_by_key(|h| h.0);
    let drops: Vec<Inst> = cells
        .iter()
        .map(|h| Inst {
            result: None,
            kind: InstKind::Call(str_drop, vec![Operand::Value(*h)]),
            origin: None,
        })
        .collect();
    release_on_exits(func, &plan.body, &drops);
    for h in cells {
        sink::sink_minted(func, &mut plan.body, plan.header, h, str_drop);
    }
}

/// `str_drop` of every hoisted cell on each edge leaving the loop: at
/// the head of an exit block the loop alone reaches, otherwise in a
/// new block spliced into the edge.
fn release_on_exits(func: &mut Function, body: &HashSet<BlockId>, drops: &[Inst]) {
    let mut preds = vec![0u32; func.blocks.len()];
    for block in &func.blocks {
        for s in successors(&block.term) {
            preds[s.0 as usize] += 1;
        }
    }
    let mut exits: Vec<(BlockId, BlockId)> = Vec::new();
    for &b in body {
        let mut seen = [None, None];
        for (slot, s) in successors(&func.blocks[b.0 as usize].term).enumerate() {
            if !body.contains(&s) && !seen.contains(&Some(s)) {
                seen[slot] = Some(s);
                exits.push((b, s));
            }
        }
    }
    exits.sort_by_key(|(f, t)| (f.0, t.0));
    for (from, to) in exits {
        if preds[to.0 as usize] == 1 {
            func.blocks[to.0 as usize]
                .insts
                .splice(0..0, drops.iter().cloned());
        } else {
            let e = new_block(func);
            func.blocks[e.0 as usize].insts = drops.to_vec();
            func.blocks[e.0 as usize].term = Terminator::Br(to);
            retarget(&mut func.blocks[from.0 as usize].term, to, e);
        }
    }
}

/// Every edge of `term` to `from` now goes to `to`.
fn retarget(term: &mut Terminator, from: BlockId, to: BlockId) {
    match term {
        Terminator::Br(t) => {
            if *t == from {
                *t = to;
            }
        }
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            if *then_blk == from {
                *then_blk = to;
            }
            if *else_blk == from {
                *else_blk = to;
            }
        }
        Terminator::Ret(_) | Terminator::Unreachable => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, IPred, Type, ValueInfo};

    fn decl(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Str,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }
    fn mk(result: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind,
            origin: None,
        }
    }
    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }
    fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }
    const CONCAT: FuncId = FuncId(0);
    const DROP: FuncId = FuncId(1);
    const SINK: FuncId = FuncId(2);

    /// bb0: %0 %1 static strs, %2 = 0, cond_br bb1 / bb3 (the guard) ·
    /// bb1 (header): `pre` · %3 = concat(%0, %1) · sink(%3) · %4 = %2
    /// + 1 · %2 = copy %4 · %5 = %2 < 10 · cond_br bb1 / bb2 · bb2:
    /// cond_br bb3 / bb4 · bb3, bb4: ret — a bottom-tested loop with
    /// two exits, one of which shares its target with the guard.
    fn rotated(pre: Vec<Inst>, left: Operand) -> Module {
        let mut header = pre;
        header.extend([
            mk(Some(3), InstKind::Call(CONCAT, vec![left, v(1)])),
            mk(None, InstKind::Call(SINK, vec![v(3)])),
            mk(
                Some(4),
                InstKind::BinOp(torajs_core::ssa::BinOp::Add, v(2), Operand::ConstI64(1)),
            ),
            mk(Some(2), InstKind::Copy(Type::I64, v(4))),
            mk(
                Some(5),
                InstKind::ICmp(IPred::Slt, v(2), Operand::ConstI64(10)),
            ),
        ]);
        let blocks = vec![
            blk(
                0,
                vec![
                    mk(
                        Some(0),
                        InstKind::StaticStrRef(torajs_core::ssa::StringId(0)),
                    ),
                    mk(
                        Some(1),
                        InstKind::StaticStrRef(torajs_core::ssa::StringId(1)),
                    ),
                    mk(Some(2), InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                ],
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(3),
                },
            ),
            blk(
                1,
                header,
                Terminator::CondBr {
                    cond: v(5),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            blk(
                2,
                vec![],
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(3),
                    else_blk: BlockId(4),
                },
            ),
            blk(3, vec![], Terminator::Ret(None)),
            blk(4, vec![], Terminator::Ret(None)),
        ];
        let ty = |t| ValueInfo { ty: t, name: None };
        Module {
            funcs: vec![
                decl("__torajs_str_concat"),
                decl("__torajs_str_drop"),
                decl("sink"),
                Function {
                    name: "main".into(),
                    params: vec![],
                    ret: Type::I64,
                    blocks,
                    values: vec![
                        ty(Type::Str),
                        ty(Type::Str),
                        ty(Type::I64),
                        ty(Type::Str),
                        ty(Type::I64),
                        ty(Type::Bool),
                    ],
                    current_origin: None,
                },
            ],
            ..Default::default()
        }
    }

    fn is_drop(inst: &Inst, h: ValueId) -> bool {
        inst.kind == InstKind::Call(DROP, vec![Operand::Value(h)])
    }

    #[test]
    fn an_invariant_concat_moves_to_a_preheader_and_is_released_on_every_exit() {
        let mut m = rotated(vec![], v(0));
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 1);
        let f = &m.funcs[3];
        let h = ValueId(6);
        // the guard now enters through the preheader
        let pre = match f.blocks[0].term {
            Terminator::CondBr { then_blk, .. } => then_blk,
            _ => unreachable!(),
        };
        assert_ne!(pre, BlockId(1));
        let p = &f.blocks[pre.0 as usize];
        assert_eq!(
            p.insts,
            vec![mk(Some(6), InstKind::Call(CONCAT, vec![v(0), v(1)]))]
        );
        assert_eq!(p.term, Terminator::Br(BlockId(1)));
        // the header mints the body's reference and every use reads the cell
        let hd = &f.blocks[1];
        assert_eq!(hd.insts[0].kind, InstKind::RcInc(Operand::Value(h)));
        assert_eq!(
            hd.insts[1].kind,
            InstKind::Call(SINK, vec![Operand::Value(h)])
        );
        // bb2 is reached only from the loop: the release goes at its head
        assert!(is_drop(&f.blocks[2].insts[0], h));
        // bb3 is shared with the guard: the exit edge got its own block
        let Terminator::CondBr { else_blk, .. } = f.blocks[1].term else {
            unreachable!()
        };
        assert_eq!(else_blk, BlockId(2));
        assert!(f.blocks[3].insts.is_empty());
        let direct_from_loop = f
            .blocks
            .iter()
            .filter(|b| b.insts.len() == 1 && is_drop(&b.insts[0], h))
            .count();
        assert_eq!(direct_from_loop, 1);
    }

    #[test]
    fn an_operand_defined_in_the_loop_keeps_the_concat_in_place() {
        // the right operand is the loop's own cell
        let mut m = rotated(vec![], v(0));
        m.funcs[3].blocks[1].insts[0].kind = InstKind::Call(CONCAT, vec![v(0), v(3)]);
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 0);
    }

    #[test]
    fn an_effect_before_the_concat_keeps_it_in_place() {
        let mut m = rotated(vec![mk(None, InstKind::Call(SINK, vec![v(0)]))], v(0));
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 0);
        // a load before it is no effect
        let mut m = rotated(vec![mk(Some(6), InstKind::Load(Type::I64, v(0), 8))], v(0));
        m.funcs[3].values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 1);
    }

    #[test]
    fn a_result_with_another_definition_keeps_the_concat_in_place() {
        // %3 is also written before the loop, the way a `let` assigned
        // in the body and read after it is
        let mut m = rotated(vec![], v(0));
        m.funcs[3].blocks[0]
            .insts
            .push(mk(Some(3), InstKind::Copy(Type::Str, v(0))));
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 0);
    }

    #[test]
    fn a_concat_reading_a_hoisted_one_moves_with_it() {
        // %6 = concat(%3, %1) right behind %3 = concat(%0, %1)
        let mut m = rotated(vec![], v(0));
        let f = &mut m.funcs[3];
        f.blocks[1]
            .insts
            .insert(1, mk(Some(6), InstKind::Call(CONCAT, vec![v(3), v(1)])));
        f.blocks[1]
            .insts
            .insert(2, mk(None, InstKind::Call(SINK, vec![v(6)])));
        f.values.push(ValueInfo {
            ty: Type::Str,
            name: None,
        });
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 2);
        let f = &m.funcs[3];
        let Terminator::CondBr { then_blk: pre, .. } = f.blocks[0].term else {
            unreachable!()
        };
        let h1 = Operand::Value(ValueId(7));
        assert_eq!(
            f.blocks[pre.0 as usize]
                .insts
                .iter()
                .map(|i| &i.kind)
                .collect::<Vec<_>>(),
            vec![
                &InstKind::Call(CONCAT, vec![v(0), v(1)]),
                &InstKind::Call(CONCAT, vec![h1, v(1)]),
            ]
        );
        // both cells are released on the exit the loop alone reaches
        let head: Vec<_> = f.blocks[2].insts.iter().take(2).collect();
        assert!(is_drop(head[0], ValueId(7)) && is_drop(head[1], ValueId(8)));
        assert!(
            !f.blocks
                .iter()
                .flat_map(|b| &b.insts)
                .any(|i| i.kind == InstKind::Call(CONCAT, vec![v(3), v(1)]))
        );
    }

    /// `rotated` with a throw probe closing the header: its throwing
    /// arm goes to bb5, which rejoins the loop at bb6 when `caught`
    /// and returns otherwise.
    fn probed(caught: bool) -> Module {
        let mut m = rotated(vec![], v(0));
        let f = &mut m.funcs[3];
        let rest = f.blocks[1].insts.split_off(2);
        let term = std::mem::replace(
            &mut f.blocks[1].term,
            Terminator::CondBr {
                cond: v(8),
                then_blk: BlockId(5),
                else_blk: BlockId(6),
            },
        );
        f.blocks[1].insts.extend([
            mk(
                Some(6),
                InstKind::GlobalRef(torajs_core::ssa::THROW_ACTIVE_SYM.into()),
            ),
            mk(Some(7), InstKind::Load(Type::I64, v(6), 0)),
            mk(
                Some(8),
                InstKind::ICmp(IPred::Ne, v(7), Operand::ConstI64(0)),
            ),
        ]);
        let catch = if caught {
            Terminator::Br(BlockId(6))
        } else {
            Terminator::Ret(None)
        };
        f.blocks.push(blk(5, vec![], catch));
        f.blocks.push(blk(6, rest, term));
        for t in [Type::Ptr, Type::I64, Type::Bool] {
            f.values.push(ValueInfo { ty: t, name: None });
        }
        m
    }

    #[test]
    fn a_loop_that_catches_inside_keeps_the_concat_in_place() {
        // every lap must raise the RangeError again for its own catch
        assert_eq!(hoist_concats(&mut probed(true)).concats_hoisted, 0);
        // a throw that leaves the loop is raised once either way
        assert_eq!(hoist_concats(&mut probed(false)).concats_hoisted, 1);
    }

    #[test]
    fn a_top_tested_loop_is_left_alone() {
        // the header holds only the test; the concat is in the body block
        let mut m = rotated(vec![], v(0));
        let f = &mut m.funcs[3];
        let body = f.blocks[1].insts.split_off(0);
        f.blocks[1].insts = vec![mk(
            Some(5),
            InstKind::ICmp(IPred::Slt, v(2), Operand::ConstI64(10)),
        )];
        f.blocks[1].term = Terminator::CondBr {
            cond: v(5),
            then_blk: BlockId(5),
            else_blk: BlockId(2),
        };
        f.blocks[0].term = Terminator::Br(BlockId(1));
        let mut body: Vec<Inst> = body
            .into_iter()
            .filter(|i| i.result != Some(ValueId(5)))
            .collect();
        body.truncate(4);
        f.blocks.push(blk(5, body, Terminator::Br(BlockId(1))));
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 0);
    }

    #[test]
    fn a_body_that_only_reads_the_cell_touches_no_header() {
        // the body reads the cell and releases it at its close
        let mut m = rotated(vec![], v(0));
        let f = &mut m.funcs[3];
        f.blocks[1].insts[1] = mk(Some(6), InstKind::Load(Type::I64, v(3), 8));
        f.blocks[1]
            .insts
            .insert(2, mk(None, InstKind::Call(DROP, vec![v(3)])));
        f.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 1);
        let f = &m.funcs[3];
        let h = ValueId(7);
        // the retain cancelled against the close's release: the header
        // holds the read alone, the exits still release the loop's own
        let hd = &f.blocks[1];
        assert_eq!(hd.insts.len(), 4);
        assert_eq!(
            hd.insts[0].kind,
            InstKind::Load(Type::I64, Operand::Value(h), 8)
        );
        assert!(!f.blocks.iter().any(|b| {
            b.insts
                .iter()
                .any(|i| i.kind == InstKind::RcInc(Operand::Value(h)))
        }));
        assert!(is_drop(&f.blocks[2].insts[0], h));
    }

    #[test]
    fn a_slot_store_swaps_only_when_the_slot_holds_another_cell() {
        // `old = slot; str_drop(old); slot = s` in the header
        let mut m = rotated(vec![], v(0));
        let f = &mut m.funcs[3];
        f.blocks[1].insts.splice(
            1..2,
            [
                mk(
                    Some(6),
                    InstKind::LoadDyn(Type::Str, v(0), Operand::ConstI64(16)),
                ),
                mk(None, InstKind::Call(DROP, vec![v(6)])),
                mk(None, InstKind::StoreDyn(v(3), v(0), Operand::ConstI64(16))),
            ],
        );
        f.values.push(ValueInfo {
            ty: Type::Str,
            name: None,
        });
        assert_eq!(hoist_concats(&mut m).concats_hoisted, 1);
        let f = &m.funcs[3];
        let (h, eq) = (ValueId(7), ValueId(8));
        let hd = &f.blocks[1];
        assert_eq!(
            hd.insts.iter().map(|i| &i.kind).collect::<Vec<_>>(),
            vec![
                &InstKind::LoadDyn(Type::Str, v(0), Operand::ConstI64(16)),
                &InstKind::ICmp(IPred::Eq, v(6), Operand::Value(h)),
            ]
        );
        let Terminator::CondBr {
            cond,
            then_blk: tail,
            else_blk: swap,
        } = hd.term
        else {
            unreachable!()
        };
        assert_eq!(cond, Operand::Value(eq));
        let s = &f.blocks[swap.0 as usize];
        assert_eq!(
            s.insts.iter().map(|i| &i.kind).collect::<Vec<_>>(),
            vec![
                &InstKind::Call(DROP, vec![v(6)]),
                &InstKind::RcInc(Operand::Value(h)),
            ]
        );
        assert_eq!(s.term, Terminator::Br(tail));
        let t = &f.blocks[tail.0 as usize];
        assert_eq!(
            t.insts[0].kind,
            InstKind::StoreDyn(Operand::Value(h), v(0), Operand::ConstI64(16))
        );
        assert_eq!(t.insts.len(), 4);
        assert!(matches!(
            t.term,
            Terminator::CondBr {
                then_blk: BlockId(1),
                ..
            }
        ));
    }
}
