//! Guarded-arm speculation — blade 3 of RFC 20260719-select-formation.
//!
//! Extends pure-diamond if-conversion to the shape float_demote's loop
//! versioning leaves behind: one pure arm P, and a guard-chain arm
//! G0→…→Gn→B where every Gi is pure defs + `CondBr{gi, cold_i, next}`
//! (a side-exit into the f64 fallback region that never fires on the
//! hot path) and the tail B carries the join Copys. Collatz parity is
//! the canonical instance: even arm = `asr`, odd arm = window guard →
//! `3n+1` → growth guard (decomposition O01, ~2.5-4 cyc/step of
//! mispredict the branch can never amortize because P(even|even)≈50%).
//!
//! Transform: hoist every pure def from both arms into the header
//! (wrap-semantics ALU only — speculation cannot trap), pair the join
//! Copys into `%m = select c, p, b`, reduce the guard conds with
//! Select-cascades (`g_or = select g1, true, g2`; `bad = select c,
//! false, g_or` so guards only fire on the G-arm path), and re-point
//! the header at `CondBr{bad, dispatch, join}` where dispatch re-splits
//! the cold targets in original guard order. The cold branch is
//! ~never-taken (perfectly predictable) — the unpredictable parity
//! branch is gone.
//!
//! v1 scope gates: cold exits must sit in the CondBr *then* slot (the
//! float_demote guard shape; no Not-inst exists to flip a cond), at
//! most 2 guards, and the whole speculated payload ≤
//! `MAX_GUARD_SPECULATED` defs.

use torajs_core::ssa::{
    Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
};

use super::{ArmPlan, SelectFormStats, plan_arm};

/// Total speculated defs across both arms (P + chain + tail). Collatz
/// parity needs 5 (asr + guard icmp + mul + add + guard icmp);
/// headroom of one more ALU op stays far under a mispredict's cost.
const MAX_GUARD_SPECULATED: usize = 6;

/// Longest supported guard chain. Each guard adds a Select to the
/// `bad` reduction and one dispatch re-split branch on the cold path.
const MAX_GUARDS: usize = 2;

pub(super) struct GuardChainPlan {
    /// pure defs along the chain (chain order), tail's included.
    speculated: Vec<Inst>,
    /// (guard cond, cold target) per chain block, in chain order.
    /// Cold is always the then-slot (v1 shape gate).
    guards: Vec<(Operand, BlockId)>,
    /// chain block indices G0..Gn (to empty after hoisting).
    chain_blocks: Vec<usize>,
    /// tail block index B (join-copy carrier).
    tail: usize,
    /// B's join copies, `(vid, ty, src)` in order.
    copies: Vec<(ValueId, Type, Operand)>,
}

/// Walk a guard chain starting at `g0`, expecting it to rejoin `join`.
/// Every chain block must be single-pred, all-speculatable, and exit
/// through `CondBr{g, cold, next}`; the walk ends at a block whose
/// terminator is `Br(join)` (the copy tail). Cold targets must be
/// single-pred so the dispatch block can take over their edge.
pub(super) fn plan_guard_chain(
    func: &Function,
    g0: BlockId,
    join: BlockId,
    pred_count: impl Fn(BlockId) -> u32,
) -> Option<GuardChainPlan> {
    let mut speculated = Vec::new();
    let mut guards = Vec::new();
    let mut chain_blocks = Vec::new();
    let mut cur = g0;
    loop {
        let bi = cur.0 as usize;
        if pred_count(cur) != 1 {
            return None;
        }
        match &func.blocks[bi].term {
            Terminator::Br(t) if *t == join => {
                // tail: copies + optional pure defs
                let ArmPlan { hoisted, copies } = plan_arm(func, bi)?;
                speculated.extend(hoisted);
                if copies.is_empty() {
                    return None; // no join value — nothing to select
                }
                return Some(GuardChainPlan {
                    speculated,
                    guards,
                    chain_blocks,
                    tail: bi,
                    copies,
                });
            }
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                if guards.len() == MAX_GUARDS {
                    return None;
                }
                let (cold, next) = (*then_blk, *else_blk);
                if cold == join || next == cold || pred_count(cold) != 1 {
                    return None;
                }
                // chain body must be pure defs only (no Copys mid-chain)
                for inst in &func.blocks[bi].insts {
                    if !(super::can_speculate(&inst.kind) && inst.result.is_some()) {
                        return None;
                    }
                }
                speculated.extend(func.blocks[bi].insts.iter().cloned());
                guards.push((cond.clone(), cold));
                chain_blocks.push(bi);
                cur = next;
            }
            _ => return None,
        }
    }
}

/// Try converting `H: CondBr{cond, P, G0}` (or mirrored) where one arm
/// is pure and the other is a guard chain. Returns true on conversion.
#[allow(clippy::too_many_arguments)]
pub(super) fn try_form_guarded(
    func: &mut Function,
    h: usize,
    cond: &Operand,
    p_id: BlockId,
    g0_id: BlockId,
    p_is_then: bool,
    pred_count: impl Fn(BlockId) -> u32,
    def_count: impl Fn(ValueId) -> u32,
    stats: &mut SelectFormStats,
) -> bool {
    let p = p_id.0 as usize;
    if pred_count(p_id) != 1 {
        return false;
    }
    let Terminator::Br(join) = func.blocks[p].term else {
        return false;
    };
    if join == p_id || join == g0_id {
        return false;
    }
    let Some(pp) = plan_arm(func, p) else {
        return false;
    };
    let Some(gp) = plan_guard_chain(func, g0_id, join, &pred_count) else {
        return false;
    };
    if gp.guards.is_empty() {
        return false; // pure chain — that's the blade-2 diamond, not ours
    }
    if pp.copies.len() != gp.copies.len()
        || pp.hoisted.len() + gp.speculated.len() > MAX_GUARD_SPECULATED
    {
        return false;
    }
    let paired = pp
        .copies
        .iter()
        .all(|(v, _, _)| gp.copies.iter().any(|(w, _, _)| w == v) && def_count(*v) == 2);
    let self_ref = pp.copies.iter().chain(gp.copies.iter()).any(
        |(_, _, src)| matches!(src, Operand::Value(x) if pp.copies.iter().any(|(v, _, _)| v == x)),
    );
    if !paired || self_ref {
        return false;
    }

    // hoist both arms' speculated defs
    stats.insts_hoisted += (pp.hoisted.len() + gp.speculated.len()) as u32;
    let mut appended: Vec<Inst> = Vec::new();
    appended.extend(pp.hoisted);
    appended.extend(gp.speculated);

    // join selects: cond true → P arm value iff P is the then arm
    for (vid, ty, p_src) in &pp.copies {
        let (_, _, g_src) = gp
            .copies
            .iter()
            .find(|(w, _, _)| w == vid)
            .expect("pairing verified above");
        let (t_src, e_src) = if p_is_then {
            (p_src.clone(), g_src.clone())
        } else {
            (g_src.clone(), p_src.clone())
        };
        appended.push(Inst {
            result: Some(*vid),
            kind: InstKind::Select(*ty, cond.clone(), t_src, e_src),
            origin: None,
        });
        stats.selects_formed += 1;
    }

    // guard reduction: g_or = g1 ∨ g2 (Select cascade), then gate it
    // to the G-arm path: bad = P-taken ? false : g_or.
    let new_bool = |func: &mut Function| {
        func.values.push(ValueInfo {
            ty: Type::Bool,
            name: None,
        });
        ValueId((func.values.len() - 1) as u32)
    };
    let g_or = if gp.guards.len() == 1 {
        gp.guards[0].0.clone()
    } else {
        let vid = new_bool(func);
        appended.push(Inst {
            result: Some(vid),
            kind: InstKind::Select(
                Type::Bool,
                gp.guards[0].0.clone(),
                Operand::ConstBool(true),
                gp.guards[1].0.clone(),
            ),
            origin: None,
        });
        Operand::Value(vid)
    };
    let bad = new_bool(func);
    let (bad_t, bad_e) = if p_is_then {
        (Operand::ConstBool(false), g_or)
    } else {
        (g_or, Operand::ConstBool(false))
    };
    appended.push(Inst {
        result: Some(bad),
        kind: InstKind::Select(Type::Bool, cond.clone(), bad_t, bad_e),
        origin: None,
    });

    // cold dispatch: one guard → its target directly; two → re-split
    // in original guard order via a fresh appended block (BlockId ==
    // position invariant, same as phi_promote's edge split).
    let dispatch = if gp.guards.len() == 1 {
        gp.guards[0].1
    } else {
        let id = BlockId(func.blocks.len() as u32);
        func.blocks.push(Block {
            id,
            insts: vec![],
            term: Terminator::CondBr {
                cond: gp.guards[0].0.clone(),
                then_blk: gp.guards[0].1,
                else_blk: gp.guards[1].1,
            },
        });
        id
    };

    func.blocks[h].insts.extend(appended);
    func.blocks[h].term = Terminator::CondBr {
        cond: Operand::Value(bad),
        then_blk: dispatch,
        else_blk: join,
    };
    // empty the consumed arm blocks; retarget their terms at the join
    // so no dead edge inflates cold targets' pred counts.
    for bi in gp.chain_blocks.iter().chain([&p, &gp.tail]) {
        func.blocks[*bi].insts.clear();
        func.blocks[*bi].term = Terminator::Br(join);
    }
    stats.guarded_converted += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::super::form_selects_in_function_for_tests as run_pass;
    use super::*;
    use torajs_core::ssa::IPred;

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
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

    fn func(values: usize, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks,
            values: (0..values)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    /// Collatz-parity shape:
    ///   b0: %1 = and %0,1 ; %2 = icmp eq %1,0 ; condbr %2, b1(P), b2(G0)
    ///   b1: %3 = ashr %0,1 ; %4 = copy %3 ; br b5
    ///   b2: %5 = icmp sgt %0, 100 ; condbr %5, b6(cold), b3
    ///   b3: %6 = mul 3,%0 ; %7 = add %6,1 ; %8 = icmp sgt %7, 900 ;
    ///       condbr %8, b7(cold), b4
    ///   b4: %4 = copy %7 ; br b5
    ///   b5: ret %4
    ///   b6/b7: cold exits
    fn parity_shape() -> Function {
        use torajs_core::ssa::BinOp::*;
        func(
            9,
            vec![
                blk(
                    0,
                    vec![
                        inst(1, InstKind::BinOp(And, v(0), c(1))),
                        inst(2, InstKind::ICmp(IPred::Eq, v(1), c(0))),
                    ],
                    Terminator::CondBr {
                        cond: v(2),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(
                    1,
                    vec![
                        inst(3, InstKind::BinOp(AShr, v(0), c(1))),
                        inst(4, InstKind::Copy(Type::I64, v(3))),
                    ],
                    Terminator::Br(BlockId(5)),
                ),
                blk(
                    2,
                    vec![inst(5, InstKind::ICmp(IPred::Sgt, v(0), c(100)))],
                    Terminator::CondBr {
                        cond: v(5),
                        then_blk: BlockId(6),
                        else_blk: BlockId(3),
                    },
                ),
                blk(
                    3,
                    vec![
                        inst(6, InstKind::BinOp(Mul, c(3), v(0))),
                        inst(7, InstKind::BinOp(Add, v(6), c(1))),
                        inst(8, InstKind::ICmp(IPred::Sgt, v(7), c(900))),
                    ],
                    Terminator::CondBr {
                        cond: v(8),
                        then_blk: BlockId(7),
                        else_blk: BlockId(4),
                    },
                ),
                blk(
                    4,
                    vec![inst(4, InstKind::Copy(Type::I64, v(7)))],
                    Terminator::Br(BlockId(5)),
                ),
                blk(5, vec![], Terminator::Ret(Some(v(4)))),
                blk(6, vec![], Terminator::Ret(Some(c(-1)))),
                blk(7, vec![], Terminator::Ret(Some(c(-2)))),
            ],
        )
    }

    #[test]
    fn parity_shape_converts_with_guard_dispatch() {
        let mut f = parity_shape();
        let mut stats = SelectFormStats::default();
        run_pass(&mut f, &mut stats);
        assert_eq!(stats.guarded_converted, 1);
        // join select + g_or + bad = 1 formed select counted for the
        // join value; g_or/bad are reduction plumbing (also Selects).
        assert_eq!(stats.selects_formed, 1);
        // header: hoisted and/icmp/ashr/icmp/mul/add/icmp + 3 selects
        let h = &f.blocks[0];
        let selects = h
            .insts
            .iter()
            .filter(|i| matches!(i.kind, InstKind::Select(..)))
            .count();
        assert_eq!(selects, 3);
        // header exits on the bad cond: then → dispatch block, else → join
        let Terminator::CondBr {
            then_blk, else_blk, ..
        } = &h.term
        else {
            panic!("header must keep a CondBr for the guard side-exit");
        };
        assert_eq!(*else_blk, BlockId(5));
        // dispatch is the appended block re-splitting both cold exits
        let d = &f.blocks[then_blk.0 as usize];
        let Terminator::CondBr {
            then_blk: c0,
            else_blk: c1,
            ..
        } = &d.term
        else {
            panic!("dispatch must re-split cold targets");
        };
        assert_eq!((*c0, *c1), (BlockId(6), BlockId(7)));
        // consumed arm blocks emptied
        for bi in [1usize, 2, 3, 4] {
            assert!(f.blocks[bi].insts.is_empty());
        }
    }

    #[test]
    fn single_guard_dispatches_directly() {
        // drop the growth guard: b3 goes straight to the copy tail
        let mut f = parity_shape();
        f.blocks[3].insts.pop(); // remove %8 icmp
        // fold b3's defs into b4 to model the one-guard shape; b3
        // becomes dead — Unreachable so its old edge doesn't inflate
        // b4's pred count.
        f.blocks[3].term = Terminator::Unreachable;
        let defs: Vec<Inst> = f.blocks[3].insts.drain(..).collect();
        for (i, d) in defs.into_iter().enumerate() {
            f.blocks[4].insts.insert(i, d);
        }
        f.blocks[2].term = Terminator::CondBr {
            cond: v(5),
            then_blk: BlockId(6),
            else_blk: BlockId(4),
        };
        let mut stats = SelectFormStats::default();
        run_pass(&mut f, &mut stats);
        assert_eq!(stats.guarded_converted, 1);
        // single guard: header's then-slot IS the cold target
        let Terminator::CondBr { then_blk, .. } = &f.blocks[0].term else {
            panic!("header keeps the guard CondBr");
        };
        assert_eq!(*then_blk, BlockId(6));
    }

    #[test]
    fn cold_in_else_slot_bails() {
        // v1 only converts cold-in-then guards
        let mut f = parity_shape();
        let Terminator::CondBr {
            then_blk, else_blk, ..
        } = &mut f.blocks[2].term
        else {
            unreachable!()
        };
        std::mem::swap(then_blk, else_blk);
        let mut stats = SelectFormStats::default();
        run_pass(&mut f, &mut stats);
        assert_eq!(stats.guarded_converted, 0);
    }

    #[test]
    fn call_in_chain_bails() {
        let mut f = parity_shape();
        f.blocks[3].insts.insert(
            0,
            inst(9, InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
        );
        f.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        let mut stats = SelectFormStats::default();
        run_pass(&mut f, &mut stats);
        assert_eq!(stats.guarded_converted, 0);
    }
}
