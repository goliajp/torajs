//! Store→load forwarding over non-escaped stack slots along extended
//! basic blocks — the degenerate straight-line case of LLVM's
//! mem2reg/SROA promotion (also fired by InstCombine's store-to-load
//! forwarding). The inliner materializes callee params as
//! `alloca + store + load` round-trips; spliced into a hot loop those cost real memory ops
//! every iteration (closure-counter +6.4% attribution, fib40 +8.3%
//! self-inline attribution — same disease). This pass forwards the
//! stored value straight to the load and deletes the round-trip.
//!
//! Scope is the extended basic block: a load forwards when a store to
//! the same (slot, offset) appears earlier in its block or in the
//! chain of blocks each entered only from the one before it — every
//! path to the load runs that store last. Reaching stores across a
//! join (the loop-φ shape: `%i`/`%sum` counters, multi-ret join
//! slots) need dominance-aware φ placement — that is the full mem2reg
//! follow-up, not this pass.
//!
//! Soundness:
//!
//! * Slots come from `rc_peephole::collect_unescaped_slots` — the
//!   address never leaves load/store address position, so no call,
//!   heap store, or dynamic store can alias it. Along an extended
//!   basic block the last store to (slot, off) is therefore the value
//!   any later load observes, with **no window-closing conditions**
//!   at all.
//! * Forwarding replaces the load's result with the stored operand
//!   everywhere (operands + terminators). The stored value's def
//!   dominates the store, the store precedes the load on the walk, and
//!   the load dominates all its uses — so the replacement is
//!   SSA-valid by transitivity (a block entered only from its
//!   predecessor is dominated by it).
//! * Store/alloca removal only happens for slots with **zero loads
//!   left anywhere in the function** (any offset). Stores are pure
//!   bit-moves (no rc traffic), so deleting them cannot change
//!   refcount behaviour; the alloca itself is then unused.
//!
//! Runs after devirt/inlining (whose param spills are the food) and
//! before the egraph pass (so forwarded constants feed const-fold /
//! GVN downstream).

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Module, Operand, Terminator, ValueId};

use crate::branch_fold::point_eval::successors;
use crate::rc_peephole::collect_unescaped_slots;
use crate::slot_cells::overlapping_cells;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SlotForwardStats {
    /// Loads replaced by the in-block stored value.
    pub loads_forwarded: u32,
    /// Stores deleted because their slot has no loads left.
    pub stores_removed: u32,
    /// Alloca slots deleted outright (no loads, stores gone).
    pub slots_removed: u32,
}

/// Run store→load forwarding over every function body.
pub fn forward_slot_loads(module: &mut Module) -> SlotForwardStats {
    let mut stats = SlotForwardStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        forward_in_function(func, &mut stats);
    }
    stats
}

/// Chase a replacement chain to its final operand (load-of-load
/// shapes resolve transitively; cycles are impossible because each
/// link strictly precedes its load in program order). Shared with
/// `mem2reg`, whose cross-block chains are acyclic for the same
/// execution-order reason.
pub(crate) fn resolve(op: &Operand, replace: &HashMap<ValueId, Operand>) -> Operand {
    let mut cur = *op;
    while let Operand::Value(v) = cur {
        match replace.get(&v) {
            Some(next) => cur = *next,
            None => break,
        }
    }
    cur
}

pub(crate) fn forward_in_function(func: &mut Function, stats: &mut SlotForwardStats) {
    let slots = collect_unescaped_slots(func);
    if slots.is_empty() {
        return;
    }
    let shared = overlapping_cells(func, &slots);
    // pass 1 — plan: walk each extended basic block tracking the last
    // stored value per (slot, off); a later load of the same key on
    // the walk forwards.
    let mut replace: HashMap<ValueId, Operand> = HashMap::default();
    let only_pred = sole_predecessors(func);
    let mut seen = vec![false; func.blocks.len()];
    for head in 0..func.blocks.len() {
        if only_pred[head].is_some() {
            continue;
        }
        let mut work = vec![(head, HashMap::default())];
        while let Some((bi, mut cur)) = work.pop() {
            if std::mem::replace(&mut seen[bi], true) {
                continue;
            }
            for inst in &func.blocks[bi].insts {
                match &inst.kind {
                    InstKind::Store(val, Operand::Value(s), off) if slots.contains(s) => {
                        cur.insert((*s, *off), resolve(val, &replace));
                    }
                    InstKind::Load(_, Operand::Value(s), off)
                        if slots.contains(s) && !shared.contains(&(*s, *off)) =>
                    {
                        if let (Some(v), Some(r)) = (cur.get(&(*s, *off)), inst.result) {
                            replace.insert(r, *v);
                        }
                    }
                    _ => {}
                }
            }
            // a successor entered only from here starts with what this
            // block left in the slots
            for s in successors(&func.blocks[bi].term) {
                let s = s.0 as usize;
                if only_pred[s] == Some(bi) {
                    work.push((s, cur.clone()));
                }
            }
        }
    }
    if !replace.is_empty() {
        apply_forwards(func, &replace);
        stats.loads_forwarded += replace.len() as u32;
    }
    // pass 3 — DSE: slots with zero remaining loads (any offset) get
    // their stores and the alloca itself removed.
    let mut loaded: HashSet<ValueId> = HashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::Load(_, Operand::Value(s), _) = &inst.kind {
                loaded.insert(*s);
            }
        }
    }
    let dead_slots: HashSet<ValueId> = slots.difference(&loaded).copied().collect();
    if dead_slots.is_empty() {
        return;
    }
    for block in func.blocks.iter_mut() {
        block.insts.retain(|inst| match &inst.kind {
            InstKind::Store(_, Operand::Value(s), _) if dead_slots.contains(s) => {
                stats.stores_removed += 1;
                false
            }
            InstKind::Alloca(_) | InstKind::AllocaBytes(_)
                if matches!(inst.result, Some(r) if dead_slots.contains(&r)) =>
            {
                stats.slots_removed += 1;
                false
            }
            _ => true,
        });
    }
}

/// Rewrite every operand through the replacement map, then drop the
/// forwarded load insts.
fn apply_forwards(func: &mut Function, replace: &HashMap<ValueId, Operand>) {
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_operands(&mut inst.kind, replace);
        }
        match &mut block.term {
            Terminator::Ret(Some(op)) => *op = resolve(op, replace),
            Terminator::CondBr { cond, .. } => *cond = resolve(cond, replace),
            _ => {}
        }
    }
    for block in func.blocks.iter_mut() {
        block
            .insts
            .retain(|inst| !matches!(inst.result, Some(r) if replace.contains_key(&r)));
    }
}

/// For each block, the one block every edge into it comes from — none
/// for the entry, a join, or a block that only branches to itself.
fn sole_predecessors(func: &Function) -> Vec<Option<usize>> {
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); func.blocks.len()];
    for (bi, block) in func.blocks.iter().enumerate() {
        for s in successors(&block.term) {
            preds[s.0 as usize].push(bi);
        }
    }
    preds
        .iter()
        .enumerate()
        .map(|(bi, p)| match p.as_slice() {
            [p] if bi != 0 && *p != bi => Some(*p),
            _ => None,
        })
        .collect()
}

/// Rewrite every value operand of `kind` through the replacement map.
/// Mirrors `rc_peephole::visit_value_operands`'s coverage but mutably;
/// kept exhaustive over operand-carrying variants via the catch-all
/// helpers below. Shared with `mem2reg`.
pub(crate) fn rewrite_operands(kind: &mut InstKind, replace: &HashMap<ValueId, Operand>) {
    let r = |op: &mut Operand| *op = resolve(op, replace);
    match kind {
        InstKind::BinOp(_, a, b) | InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            r(a);
            r(b);
        }
        InstKind::Call(_, args) => args.iter_mut().for_each(r),
        InstKind::CallIndirect(_, p, args) => {
            r(p);
            args.iter_mut().for_each(r);
        }
        InstKind::Load(_, p, _)
        | InstKind::RcInc(p)
        | InstKind::RcIncAny(p)
        | InstKind::RcDecAny(p)
        | InstKind::BoxAny(_, p)
        | InstKind::AnyIsNumber(p)
        | InstKind::AnyIsInt32(p)
        | InstKind::AnyToInt32(p)
        | InstKind::BoxInt32(p)
        | InstKind::BoxF64(p) => r(p),
        InstKind::AnyToF64(v, f) => {
            r(v);
            r(f);
        }
        InstKind::Store(v, p, _) => {
            r(v);
            r(p);
        }
        InstKind::LoadDyn(_, p, o) => {
            r(p);
            r(o);
        }
        InstKind::StoreDyn(v, p, o) => {
            r(v);
            r(p);
            r(o);
        }
        InstKind::LoadDynScaled8(_, p, i) => {
            r(p);
            r(i);
        }
        InstKind::LoadU8Dyn(p, i) | InstKind::LoadU16Dyn(p, i) => {
            r(p);
            r(i);
        }
        InstKind::StoreDynScaled8(v, p, i) => {
            r(v);
            r(p);
            r(i);
        }
        InstKind::Select(_, c, t, e) => {
            r(c);
            r(t);
            r(e);
        }
        InstKind::CtpopRangeSum(s, b, a) => {
            r(s);
            r(b);
            r(a);
        }
        InstKind::Vec(vi) => vi.operands_mut().into_iter().flatten().for_each(r),
        InstKind::SiToFp(a)
        | InstKind::FpToSi(a)
        | InstKind::FpToInt32(a)
        | InstKind::ZExtBoolToI64(a)
        | InstKind::ZExtI32ToI64(a)
        | InstKind::BitCastF64ToI64(a)
        | InstKind::BitCastI64ToF64(a)
        | InstKind::IntToPtr(a)
        | InstKind::PtrToInt(a)
        | InstKind::TruncI64ToBool(a)
        | InstKind::Identity(a)
        | InstKind::Neg(a)
        | InstKind::Ctpop(a)
        | InstKind::Cttz(a)
        | InstKind::Copy(_, a)
        | InstKind::Suspend(_, _, a) => r(a),
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_)
        | InstKind::ThrowSignal => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, FuncId, Inst, Terminator, Type, ValueInfo};

    fn val(result: u32, ty: Type, kind: InstKind, values: &mut Vec<ValueInfo>) -> Inst {
        assert_eq!(values.len(), result as usize);
        values.push(ValueInfo { ty, name: None });
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    fn module_one_block(insts: Vec<Inst>, values: Vec<ValueInfo>) -> Module {
        Module {
            funcs: vec![Function {
                name: "main".into(),
                params: vec![],
                ret: Type::Void,
                blocks: vec![Block {
                    id: BlockId(0),
                    insts,
                    term: Terminator::Ret(None),
                }],
                values,
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    fn body(m: &Module) -> &[Inst] {
        &m.funcs[0].blocks[0].insts
    }

    #[test]
    fn param_spill_round_trip_collapses() {
        // the inline-param shape: alloca; store v; load → use.
        let mut vals = vec![];
        let insts = vec![
            val(
                0,
                Type::I64,
                InstKind::BinOp(
                    torajs_core::ssa::BinOp::Add,
                    Operand::ConstI64(1),
                    Operand::ConstI64(2),
                ),
                &mut vals,
            ),
            val(1, Type::Ptr, InstKind::Alloca(Type::I64), &mut vals),
            void(InstKind::Store(
                Operand::Value(ValueId(0)),
                Operand::Value(ValueId(1)),
                0,
            )),
            val(
                2,
                Type::I64,
                InstKind::Load(Type::I64, Operand::Value(ValueId(1)), 0),
                &mut vals,
            ),
            val(
                3,
                Type::I64,
                InstKind::BinOp(
                    torajs_core::ssa::BinOp::Add,
                    Operand::Value(ValueId(2)),
                    Operand::ConstI64(5),
                ),
                &mut vals,
            ),
        ];
        let mut m = module_one_block(insts, vals);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 1);
        assert_eq!(stats.stores_removed, 1);
        assert_eq!(stats.slots_removed, 1);
        // alloca + store + load all gone; the add reads %0 directly.
        assert_eq!(body(&m).len(), 2);
        let InstKind::BinOp(_, a, _) = &body(&m)[1].kind else {
            panic!("expected binop")
        };
        assert!(matches!(a, Operand::Value(ValueId(0))));
    }

    #[test]
    fn const_store_forwards_as_const() {
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::Ptr, InstKind::Alloca(Type::I64), &mut vals),
            void(InstKind::Store(
                Operand::ConstI64(7),
                Operand::Value(ValueId(0)),
                0,
            )),
            val(
                1,
                Type::I64,
                InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            void(InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(1))])),
        ];
        let mut m = module_one_block(insts, vals);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 1);
        let InstKind::Call(_, args) = &body(&m)[0].kind else {
            panic!("expected call")
        };
        assert!(matches!(args[0], Operand::ConstI64(7)));
    }

    fn slot() -> Inst {
        Inst {
            result: Some(ValueId(0)),
            kind: InstKind::Alloca(Type::I64),
            origin: None,
        }
    }

    fn st(k: i64) -> Inst {
        void(InstKind::Store(
            Operand::ConstI64(k),
            Operand::Value(ValueId(0)),
            0,
        ))
    }

    fn ld(r: u32) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind: InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
            origin: None,
        }
    }

    fn cbr(t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    fn ret(r: u32) -> Terminator {
        Terminator::Ret(Some(Operand::Value(ValueId(r))))
    }

    /// One function over blocks 0..n; value 0 is the slot, 1..4 i64.
    fn cfg(blocks: Vec<(Vec<Inst>, Terminator)>) -> Module {
        let mut values = vec![
            ValueInfo {
                ty: Type::I64,
                name: None,
            };
            4
        ];
        values[0].ty = Type::Ptr;
        let blocks = blocks
            .into_iter()
            .enumerate()
            .map(|(i, (insts, term))| Block {
                id: BlockId(i as u32),
                insts,
                term,
            })
            .collect();
        Module {
            funcs: vec![Function {
                name: "main".into(),
                params: vec![],
                ret: Type::I64,
                blocks,
                values,
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    fn ret_of(m: &Module, b: usize) -> Operand {
        match m.funcs[0].blocks[b].term {
            Terminator::Ret(Some(op)) => op,
            _ => panic!("expected ret"),
        }
    }

    #[test]
    fn load_after_a_join_not_forwarded() {
        // bb0 -> bb1 / bb2 -> bb3: the load in bb3 has two predecessors
        let mut m = cfg(vec![
            (vec![slot(), st(3)], cbr(1, 2)),
            (vec![], Terminator::Br(BlockId(3))),
            (vec![], Terminator::Br(BlockId(3))),
            (vec![ld(1)], ret(1)),
        ]);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats, SlotForwardStats::default());
        assert_eq!(m.funcs[0].blocks[0].insts.len(), 2);
        assert_eq!(m.funcs[0].blocks[3].insts.len(), 1);
    }

    #[test]
    fn store_reaches_a_successor_entered_only_from_its_block() {
        // bb1 (sole pred bb0) reads 3 and stores 4; bb3 (sole pred bb1)
        // reads 4; bb2 (sole pred bb0) reads 3
        let mut m = cfg(vec![
            (vec![slot(), st(3)], cbr(1, 2)),
            (vec![ld(1), st(4)], Terminator::Br(BlockId(3))),
            (vec![ld(2)], ret(2)),
            (vec![ld(3)], ret(3)),
        ]);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 3);
        assert!(matches!(ret_of(&m, 2), Operand::ConstI64(3)));
        assert!(matches!(ret_of(&m, 3), Operand::ConstI64(4)));
    }

    #[test]
    fn loop_header_load_not_forwarded() {
        // bb1's predecessors are the entry and the latch bb2, which
        // stores: the header load keeps reading memory, and so does
        // the exit bb3 entered only from the header
        let mut m = cfg(vec![
            (vec![slot(), st(0)], Terminator::Br(BlockId(1))),
            (vec![ld(1)], cbr(2, 3)),
            (vec![st(5)], Terminator::Br(BlockId(1))),
            (vec![ld(2)], ret(2)),
        ]);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats, SlotForwardStats::default());
        assert!(matches!(ret_of(&m, 3), Operand::Value(ValueId(2))));
    }

    #[test]
    fn self_loop_block_starts_empty_but_hands_its_stores_on() {
        // bb1 branches to itself: its load sees bb0's store on the first
        // pass and its own after that, so it stays; bb2, entered only
        // from bb1, reads bb1's last store
        let mut m = cfg(vec![
            (vec![slot(), st(1)], Terminator::Br(BlockId(1))),
            (vec![ld(1), st(2)], cbr(1, 2)),
            (vec![ld(2)], ret(2)),
        ]);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 1);
        assert!(matches!(
            m.funcs[0].blocks[1].insts[0].kind,
            InstKind::Load(..)
        ));
        assert!(matches!(ret_of(&m, 2), Operand::ConstI64(2)));
    }

    #[test]
    fn slot_with_only_stores_is_removed_without_a_forward() {
        let mut m = cfg(vec![(vec![slot(), st(1), st(2)], Terminator::Ret(None))]);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 0);
        assert_eq!((stats.stores_removed, stats.slots_removed), (2, 1));
        assert!(m.funcs[0].blocks[0].insts.is_empty());
    }

    #[test]
    fn second_store_shadows_first() {
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::Ptr, InstKind::Alloca(Type::I64), &mut vals),
            void(InstKind::Store(
                Operand::ConstI64(1),
                Operand::Value(ValueId(0)),
                0,
            )),
            void(InstKind::Store(
                Operand::ConstI64(2),
                Operand::Value(ValueId(0)),
                0,
            )),
            val(
                1,
                Type::I64,
                InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            void(InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(1))])),
        ];
        let mut m = module_one_block(insts, vals);
        forward_slot_loads(&mut m);
        let InstKind::Call(_, args) = &body(&m)[0].kind else {
            panic!("expected call")
        };
        assert!(matches!(args[0], Operand::ConstI64(2)));
    }

    #[test]
    fn partial_load_keeps_slot_alive() {
        // one load forwards (same block), another sits behind a join
        // — stores must NOT be deleted (that load still reads them).
        let mut m = cfg(vec![
            (
                vec![
                    slot(),
                    st(3),
                    ld(1),
                    void(InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(1))])),
                ],
                cbr(1, 2),
            ),
            (vec![], Terminator::Br(BlockId(3))),
            (vec![], Terminator::Br(BlockId(3))),
            (vec![ld(2)], ret(2)),
        ]);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 1);
        assert_eq!(stats.stores_removed, 0);
        assert_eq!(stats.slots_removed, 0);
        // bb3's load survives; bb0's forwarded load is gone.
        assert_eq!(m.funcs[0].blocks[3].insts.len(), 1);
        assert!(
            m.funcs[0].blocks[0]
                .insts
                .iter()
                .all(|i| !matches!(i.kind, InstKind::Load(..)))
        );
    }

    #[test]
    fn load_of_load_chain_resolves() {
        // slot B stores the result of a forwarded load from slot A —
        // the chain resolves to A's stored value.
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::Ptr, InstKind::Alloca(Type::I64), &mut vals),
            void(InstKind::Store(
                Operand::ConstI64(9),
                Operand::Value(ValueId(0)),
                0,
            )),
            val(
                1,
                Type::I64,
                InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            val(2, Type::Ptr, InstKind::Alloca(Type::I64), &mut vals),
            void(InstKind::Store(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(2)),
                0,
            )),
            val(
                3,
                Type::I64,
                InstKind::Load(Type::I64, Operand::Value(ValueId(2)), 0),
                &mut vals,
            ),
            void(InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(3))])),
        ];
        let mut m = module_one_block(insts, vals);
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 2);
        assert_eq!(stats.slots_removed, 2);
        let InstKind::Call(_, args) = &body(&m)[0].kind else {
            panic!("expected call")
        };
        assert!(matches!(args[0], Operand::ConstI64(9)));
        assert_eq!(body(&m).len(), 1);
    }

    #[test]
    fn operand_rewrite_covers_visit_value_operands() {
        // every InstKind variant that visit_value_operands reports a
        // value for must also be rewritten by rewrite_operands —
        // otherwise a forwarded load leaves a dangling ValueId behind.
        // Representative check over the operand-carrying variants used
        // by lowering around slots: covered structurally by the match
        // in rewrite_operands; this test pins the Store-value path
        // (the easiest to miss).
        let mut vals = vec![];
        let insts = vec![
            val(0, Type::Ptr, InstKind::Alloca(Type::I64), &mut vals),
            void(InstKind::Store(
                Operand::ConstI64(4),
                Operand::Value(ValueId(0)),
                0,
            )),
            val(
                1,
                Type::I64,
                InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                &mut vals,
            ),
            val(2, Type::Ptr, InstKind::Alloca(Type::I64), &mut vals),
            // the forwarded %1 feeds a store value AND a dyn offset
            void(InstKind::Store(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(2)),
                0,
            )),
            void(InstKind::StoreDyn(
                Operand::Value(ValueId(1)),
                Operand::Value(ValueId(2)),
                Operand::Value(ValueId(1)),
            )),
        ];
        let mut m = module_one_block(insts, vals);
        forward_slot_loads(&mut m);
        for inst in body(&m) {
            crate::rc_peephole::visit_value_operands(&inst.kind, |v| {
                assert_ne!(
                    v,
                    ValueId(1),
                    "dangling forwarded load use in {:?}",
                    inst.kind
                );
            });
        }
    }
}
