//! Block-local store→load forwarding over non-escaped stack slots —
//! the degenerate single-block case of LLVM's mem2reg/SROA promotion
//! (also fired by InstCombine's store-to-load forwarding). The
//! inliner materializes callee params as `alloca + store + load`
//! round-trips; spliced into a hot loop those cost real memory ops
//! every iteration (closure-counter +6.4% attribution, fib40 +8.3%
//! self-inline attribution — same disease). This pass forwards the
//! stored value straight to the load and deletes the round-trip.
//!
//! Scope deliberately block-local: a load only forwards when a store
//! to the same (slot, offset) appears earlier **in the same block**.
//! Cross-block reaching stores (the loop-φ shape: `%i`/`%sum`
//! counters, multi-ret join slots) need dominance-aware φ placement —
//! that is the full mem2reg follow-up, not this pass.
//!
//! Soundness:
//!
//! * Slots come from `rc_peephole::collect_unescaped_slots` — the
//!   address never leaves load/store address position, so no call,
//!   heap store, or dynamic store can alias it. Within a block the
//!   last store to (slot, off) is therefore the value any later load
//!   observes, with **no window-closing conditions** at all.
//! * Forwarding replaces the load's result with the stored operand
//!   everywhere (operands + terminators). The stored value's def
//!   dominates the store, the store precedes the load in-block, and
//!   the load dominates all its uses — so the replacement is
//!   SSA-valid by transitivity.
//! * Store/alloca removal only happens for slots with **zero loads
//!   left anywhere in the function** (any offset). Stores are pure
//!   bit-moves (no rc traffic), so deleting them cannot change
//!   refcount behaviour; the alloca itself is then unused.
//!
//! Runs after devirt/inlining (whose param spills are the food) and
//! before the egraph pass (so forwarded constants feed const-fold /
//! GVN downstream).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Module, Operand, Terminator, ValueId};

use crate::rc_peephole::collect_unescaped_slots;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SlotForwardStats {
    /// Loads replaced by the in-block stored value.
    pub loads_forwarded: u32,
    /// Stores deleted because their slot has no loads left.
    pub stores_removed: u32,
    /// Alloca slots deleted outright (no loads, stores gone).
    pub slots_removed: u32,
}

/// Run block-local store→load forwarding over every function body.
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

fn forward_in_function(func: &mut Function, stats: &mut SlotForwardStats) {
    let slots = collect_unescaped_slots(func);
    if slots.is_empty() {
        return;
    }
    // pass 1 — plan: walk each block tracking the last stored value
    // per (slot, off); a later in-block load of the same key forwards.
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();
    for block in &func.blocks {
        let mut cur: HashMap<(ValueId, u64), Operand> = HashMap::new();
        for inst in &block.insts {
            match &inst.kind {
                InstKind::Store(val, Operand::Value(s), off) if slots.contains(s) => {
                    cur.insert((*s, *off), resolve(val, &replace));
                }
                InstKind::Load(_, Operand::Value(s), off) if slots.contains(s) => {
                    if let (Some(v), Some(r)) = (cur.get(&(*s, *off)), inst.result) {
                        replace.insert(r, *v);
                    }
                }
                _ => {}
            }
        }
    }
    if replace.is_empty() {
        return;
    }
    // pass 2 — apply: rewrite every operand through the replacement
    // map, then drop the forwarded load insts.
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_operands(&mut inst.kind, &replace);
        }
        match &mut block.term {
            Terminator::Ret(Some(op)) => *op = resolve(op, &replace),
            Terminator::CondBr { cond, .. } => *cond = resolve(cond, &replace),
            _ => {}
        }
    }
    for block in func.blocks.iter_mut() {
        block
            .insts
            .retain(|inst| !matches!(inst.result, Some(r) if replace.contains_key(&r)));
    }
    stats.loads_forwarded += replace.len() as u32;
    // pass 3 — DSE: slots with zero remaining loads (any offset) get
    // their stores and the alloca itself removed.
    let mut loaded: HashSet<ValueId> = HashSet::new();
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
        InstKind::Load(_, p, _) => r(p),
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
        InstKind::LoadU8Dyn(p, i) => {
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
        InstKind::SiToFp(a)
        | InstKind::FpToSi(a)
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
        | InstKind::Copy(_, a) => r(a),
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_) => {}
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

    #[test]
    fn cross_block_load_not_forwarded() {
        // store in bb0, load in bb1 — out of scope for the
        // block-local pass, slot must survive untouched.
        let mut vals = vec![];
        vals.push(ValueInfo {
            ty: Type::Ptr,
            name: None,
        });
        vals.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        let m_funcs = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        Inst {
                            result: Some(ValueId(0)),
                            kind: InstKind::Alloca(Type::I64),
                            origin: None,
                        },
                        void(InstKind::Store(
                            Operand::ConstI64(3),
                            Operand::Value(ValueId(0)),
                            0,
                        )),
                    ],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: Some(ValueId(1)),
                        kind: InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                        origin: None,
                    }],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                },
            ],
            values: vals,
            current_origin: None,
        };
        let mut m = Module {
            funcs: vec![m_funcs],
            ..Default::default()
        };
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats, SlotForwardStats::default());
        assert_eq!(m.funcs[0].blocks[0].insts.len(), 2);
        assert_eq!(m.funcs[0].blocks[1].insts.len(), 1);
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
        // one load forwards (same block), another sits in a second
        // block — stores must NOT be deleted (cross-block load still
        // reads them).
        let mut vals = vec![];
        vals.push(ValueInfo {
            ty: Type::Ptr,
            name: None,
        });
        vals.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        vals.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        let f = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        Inst {
                            result: Some(ValueId(0)),
                            kind: InstKind::Alloca(Type::I64),
                            origin: None,
                        },
                        void(InstKind::Store(
                            Operand::ConstI64(3),
                            Operand::Value(ValueId(0)),
                            0,
                        )),
                        Inst {
                            result: Some(ValueId(1)),
                            kind: InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                            origin: None,
                        },
                        void(InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(1))])),
                    ],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: Some(ValueId(2)),
                        kind: InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0),
                        origin: None,
                    }],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
                },
            ],
            values: vals,
            current_origin: None,
        };
        let mut m = Module {
            funcs: vec![f],
            ..Default::default()
        };
        let stats = forward_slot_loads(&mut m);
        assert_eq!(stats.loads_forwarded, 1);
        assert_eq!(stats.stores_removed, 0);
        assert_eq!(stats.slots_removed, 0);
        // bb1's load survives; bb0's forwarded load is gone.
        assert_eq!(m.funcs[0].blocks[1].insts.len(), 1);
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
