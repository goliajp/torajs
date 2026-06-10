//! Redundant ToInt32 sext-pair elimination — the minimal known-bits
//! subset (LLVM ValueTracking `ComputeNumSignBits` analogue) over the
//! post-φ-promotion SSA. The JS ToInt32 alignment normalizes every
//! bitwise operand with an explicit `shl 32` + `ashr 32` pair
//! (`lower_bitwise_int32`); the self codegen emits those literally, so
//! in a hot loop they are real machine instructions (popcount +17.1%,
//! four shifts per iteration). Two semantics-preserving rewrites
//! recover them:
//!
//! * **Transpose (R1)** — `and/or/xor(sx(a), sx(b)) → sx(and/or/xor(a, b))`
//!   where `sx(v) = ashr(shl(v, 32), 32)`. Unconditional bit-level
//!   identity: the low 32 result bits depend only on the low 32
//!   operand bits, and the high 32 bits of both forms are the
//!   replicated bit-31 of the low result. Halves the pair count on
//!   every two-operand bitwise op (popcount: 4 → 2 shifts/iter). An
//!   operand also qualifies without its own pair when it is already
//!   provably sext-32 (in-range ConstI64 or a lattice fact).
//! * **Elide (R2)** — `ashr(shl(x, 32), 32)` where `x` is already
//!   provably sign-extended-from-32 collapses to `x`. Facts come from
//!   a conservative single-forward-pass lattice: ConstI64 in i32
//!   range, results of sext pairs, and/or/xor of two sext-32 values,
//!   ashr of a sext-32 value, single-def Copy/Identity of a sext-32
//!   value. No fixpoint of its own: loop-carried (multi-def Copy)
//!   values stay unknown unless the interval analysis (`interval`
//!   module, RFC 20260611) seeded them as provably in-i32-range.
//!   Shapes that can leave i32 range and *need* the wrap (add/sub
//!   results — `ToInt32(INT32_MIN - 1)` must wrap) are never in this
//!   pass's own lattice (the interval seed proves range, which is
//!   strictly stronger).
//!
//! Runs after `phi_promote` (final canonical shape, right before the
//! SSA dump / rc_peephole). Orphaned shift insts left by either
//! rewrite are swept by a local pure-shift DCE.
//! `TORAJS_SEXT_ELIDE_OFF=1` skips (bisect gate),
//! `TORAJS_SEXT_ELIDE_STATS=1` dumps counters.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
};

use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::{resolve, rewrite_operands};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SextElideStats {
    /// Bitwise ops rewritten from operand-side pairs to one result-side pair.
    pub transposed: u32,
    /// Sext pairs collapsed to their already-sext-32 source.
    pub pairs_elided: u32,
    /// Orphaned shl/ashr insts swept after the rewrites.
    pub dead_shifts_removed: u32,
}

/// Run sext-pair elimination over every function body. `seeds` is
/// index-aligned with `module.funcs` (empty slice = no seeds): extra
/// sext-32 facts from the interval analysis, which can certify
/// loop-carried cells this pass's own one-pass lattice must treat as
/// unknown.
pub fn elide_sext_pairs(module: &mut Module, seeds: &[HashSet<ValueId>]) -> SextElideStats {
    let mut stats = SextElideStats::default();
    let empty = HashSet::new();
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        if func.is_declaration() {
            continue;
        }
        elide_in_function(func, seeds.get(fi).unwrap_or(&empty), &mut stats);
    }
    stats
}

/// Operand is a compile-time-provable sext-32 value: a ConstI64 whose
/// i64 form equals its own low-32 sign extension, or a lattice fact.
fn op_sext32(op: &Operand, facts: &HashSet<ValueId>) -> bool {
    match op {
        Operand::ConstI64(c) => *c >= i32::MIN as i64 && *c <= i32::MAX as i64,
        Operand::Value(v) => facts.contains(v),
        _ => false,
    }
}

/// `v` is the ashr half of `ashr(shl(src, 32), 32)` → the raw `src`.
fn sxpair_src(v: ValueId, defs: &HashMap<ValueId, InstKind>) -> Option<Operand> {
    if let Some(InstKind::BinOp(BinOp::AShr, Operand::Value(u), Operand::ConstI64(32))) =
        defs.get(&v)
    {
        if let Some(InstKind::BinOp(BinOp::Shl, src, Operand::ConstI64(32))) = defs.get(u) {
            return Some(*src);
        }
    }
    None
}

fn push_i64_value(values: &mut Vec<ValueInfo>) -> ValueId {
    let id = ValueId(values.len() as u32);
    values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    id
}

fn elide_in_function(func: &mut Function, seed: &HashSet<ValueId>, stats: &mut SextElideStats) {
    // def summaries for single-def values; multi-def values (post-φ
    // Copy cells) are excluded from the map and conservatively unknown
    // unless the interval analysis seeded them as provably sext-32.
    let mut defs: HashMap<ValueId, InstKind> = HashMap::new();
    let mut multi: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                if defs.insert(r, inst.kind.clone()).is_some() {
                    multi.insert(r);
                }
            }
        }
    }
    for v in &multi {
        defs.remove(v);
    }

    let mut facts: HashSet<ValueId> = seed.clone();
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();

    for bi in 0..func.blocks.len() {
        let old = std::mem::take(&mut func.blocks[bi].insts);
        let mut out: Vec<Inst> = Vec::with_capacity(old.len());
        for inst in old {
            let Some(r) = inst.result else {
                out.push(inst);
                continue;
            };
            // R2 — the inst is itself the ashr half of a sext pair.
            if !multi.contains(&r) {
                if let InstKind::BinOp(BinOp::AShr, Operand::Value(u), Operand::ConstI64(32)) =
                    inst.kind
                {
                    if let Some(InstKind::BinOp(BinOp::Shl, src, Operand::ConstI64(32))) =
                        defs.get(&u)
                    {
                        let src = *src;
                        if op_sext32(&src, &facts) {
                            // pair collapses to its source; the orphan
                            // shl is swept by the DCE below.
                            replace.insert(r, src);
                            facts.insert(r);
                            stats.pairs_elided += 1;
                            continue;
                        }
                        // un-elidable pair still produces a sext-32 value
                        facts.insert(r);
                        out.push(inst);
                        continue;
                    }
                }
            }
            // R1 — transpose operand-side pairs into one result-side pair.
            if let InstKind::BinOp(bop @ (BinOp::And | BinOp::Or | BinOp::Xor), a, b) = inst.kind {
                // qualifying operand → (operand to use, had its own pair).
                // Resolve through R2's replacements first: an operand
                // whose pair already elided is its raw (sext-32) source,
                // so it must qualify pair-free — re-transposing it would
                // recreate the pair R2 just removed.
                let qualify = |op: &Operand| -> Option<(Operand, bool)> {
                    let op = resolve(op, &replace);
                    if let Operand::Value(v) = op {
                        if let Some(src) = sxpair_src(v, &defs) {
                            return Some((src, true));
                        }
                    }
                    if op_sext32(&op, &facts) {
                        return Some((op, false));
                    }
                    None
                };
                match (qualify(&a), qualify(&b)) {
                    (Some((qa, pa)), Some((qb, pb))) if pa || pb => {
                        let t1 = push_i64_value(&mut func.values);
                        let t2 = push_i64_value(&mut func.values);
                        let k1 = InstKind::BinOp(bop, qa, qb);
                        let k2 =
                            InstKind::BinOp(BinOp::Shl, Operand::Value(t1), Operand::ConstI64(32));
                        let k3 =
                            InstKind::BinOp(BinOp::AShr, Operand::Value(t2), Operand::ConstI64(32));
                        out.push(Inst {
                            result: Some(t1),
                            kind: k1.clone(),
                            origin: inst.origin,
                        });
                        out.push(Inst {
                            result: Some(t2),
                            kind: k2.clone(),
                            origin: inst.origin,
                        });
                        out.push(Inst {
                            result: Some(r),
                            kind: k3.clone(),
                            origin: inst.origin,
                        });
                        defs.insert(t1, k1);
                        defs.insert(t2, k2);
                        if !multi.contains(&r) {
                            defs.insert(r, k3);
                            facts.insert(r);
                        }
                        stats.transposed += 1;
                        continue;
                    }
                    (Some(_), Some(_)) => {
                        // both operands already sext-32 with no pair to
                        // move — the result closes over itself.
                        if !multi.contains(&r) {
                            facts.insert(r);
                        }
                        out.push(inst);
                        continue;
                    }
                    _ => {
                        out.push(inst);
                        continue;
                    }
                }
            }
            // fact propagation for the remaining shapes
            if !multi.contains(&r) {
                match &inst.kind {
                    InstKind::BinOp(BinOp::AShr, a, Operand::ConstI64(k))
                        if (0..64).contains(k) && op_sext32(a, &facts) =>
                    {
                        facts.insert(r);
                    }
                    InstKind::Copy(_, a) | InstKind::Identity(a) if op_sext32(a, &facts) => {
                        facts.insert(r);
                    }
                    _ => {}
                }
            }
            out.push(inst);
        }
        func.blocks[bi].insts = out;
    }

    if !replace.is_empty() {
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
    }

    // DCE — orphaned pure shifts left behind by either rewrite. A dead
    // ashr frees its shl on the next round; loop until stable.
    loop {
        let mut used: HashSet<ValueId> = HashSet::new();
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| {
                    used.insert(v);
                });
            }
            match &block.term {
                Terminator::Ret(Some(Operand::Value(v))) => {
                    used.insert(*v);
                }
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                } => {
                    used.insert(*v);
                }
                _ => {}
            }
        }
        let mut removed = 0u32;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|inst| {
                let dead = matches!(
                    (&inst.result, &inst.kind),
                    (Some(r), InstKind::BinOp(BinOp::Shl | BinOp::AShr, _, _))
                        if !used.contains(r)
                );
                if dead {
                    removed += 1;
                }
                !dead
            });
        }
        if removed == 0 {
            break;
        }
        stats.dead_shifts_removed += removed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Terminator};

    fn val(result: u32, kind: InstKind, values: &mut Vec<ValueInfo>) -> Inst {
        assert_eq!(values.len(), result as usize);
        values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn binop(op: BinOp, a: Operand, b: Operand) -> InstKind {
        InstKind::BinOp(op, a, b)
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn module_one_block(insts: Vec<Inst>, values: Vec<ValueInfo>, term: Terminator) -> Module {
        Module {
            funcs: vec![Function {
                name: "main".into(),
                params: vec![],
                ret: Type::I64,
                blocks: vec![Block {
                    id: BlockId(0),
                    insts,
                    term,
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
    fn popcount_shape_transposes_to_one_pair() {
        // %0 = unknown; %1 = %0 - 1; and(sx(%0), sx(%1)) — the
        // popcount hot-loop shape. Both operand pairs fold into one
        // result-side pair; the four orphan shifts die.
        let mut vals = vec![];
        let insts = vec![
            val(0, binop(BinOp::Add, c(100), c(3)), &mut vals),
            val(1, binop(BinOp::Sub, v(0), c(1)), &mut vals),
            val(2, binop(BinOp::Shl, v(0), c(32)), &mut vals),
            val(3, binop(BinOp::AShr, v(2), c(32)), &mut vals),
            val(4, binop(BinOp::Shl, v(1), c(32)), &mut vals),
            val(5, binop(BinOp::AShr, v(4), c(32)), &mut vals),
            val(6, binop(BinOp::And, v(3), v(5)), &mut vals),
        ];
        let mut m = module_one_block(insts, vals, Terminator::Ret(Some(v(6))));
        let stats = elide_sext_pairs(&mut m, &[]);
        assert_eq!(stats.transposed, 1);
        assert_eq!(stats.pairs_elided, 0);
        assert_eq!(stats.dead_shifts_removed, 4);
        // add, sub, and(raw, raw), shl, ashr(%6)
        assert_eq!(body(&m).len(), 5);
        let InstKind::BinOp(BinOp::And, a, b) = &body(&m)[2].kind else {
            panic!("expected raw and, got {:?}", body(&m)[2].kind);
        };
        assert_eq!((*a, *b), (v(0), v(1)));
        assert_eq!(body(&m)[4].result, Some(ValueId(6)));
        assert!(matches!(
            body(&m)[4].kind,
            InstKind::BinOp(BinOp::AShr, _, Operand::ConstI64(32))
        ));
    }

    #[test]
    fn in_range_const_operand_qualifies() {
        // ~n lowering: xor(sx(%0), -1) — const side needs no pair.
        let mut vals = vec![];
        let insts = vec![
            val(0, binop(BinOp::Add, c(7), c(9)), &mut vals),
            val(1, binop(BinOp::Shl, v(0), c(32)), &mut vals),
            val(2, binop(BinOp::AShr, v(1), c(32)), &mut vals),
            val(3, binop(BinOp::Xor, v(2), c(-1)), &mut vals),
        ];
        let mut m = module_one_block(insts, vals, Terminator::Ret(Some(v(3))));
        let stats = elide_sext_pairs(&mut m, &[]);
        assert_eq!(stats.transposed, 1);
        assert_eq!(stats.dead_shifts_removed, 2);
        let InstKind::BinOp(BinOp::Xor, a, b) = &body(&m)[1].kind else {
            panic!("expected raw xor, got {:?}", body(&m)[1].kind);
        };
        assert_eq!((*a, *b), (v(0), c(-1)));
    }

    #[test]
    fn out_of_range_const_blocks_fire() {
        // and(sx(%0), 2^31) — the const is not sext-32; transposing
        // would change the result's high bits. Must not fire.
        let mut vals = vec![];
        let insts = vec![
            val(0, binop(BinOp::Add, c(7), c(9)), &mut vals),
            val(1, binop(BinOp::Shl, v(0), c(32)), &mut vals),
            val(2, binop(BinOp::AShr, v(1), c(32)), &mut vals),
            val(3, binop(BinOp::And, v(2), c(1_i64 << 31)), &mut vals),
        ];
        let mut m = module_one_block(insts, vals, Terminator::Ret(Some(v(3))));
        let stats = elide_sext_pairs(&mut m, &[]);
        assert_eq!(stats, SextElideStats::default());
        assert_eq!(body(&m).len(), 4);
    }

    #[test]
    fn pair_of_pair_elides() {
        // sx(sx(%0)) — the outer pair's source is a pair result
        // (sext-32 fact) and collapses to it.
        let mut vals = vec![];
        let insts = vec![
            val(0, binop(BinOp::Add, c(7), c(9)), &mut vals),
            val(1, binop(BinOp::Shl, v(0), c(32)), &mut vals),
            val(2, binop(BinOp::AShr, v(1), c(32)), &mut vals),
            val(3, binop(BinOp::Shl, v(2), c(32)), &mut vals),
            val(4, binop(BinOp::AShr, v(3), c(32)), &mut vals),
        ];
        let mut m = module_one_block(insts, vals, Terminator::Ret(Some(v(4))));
        let stats = elide_sext_pairs(&mut m, &[]);
        assert_eq!(stats.pairs_elided, 1);
        assert_eq!(stats.dead_shifts_removed, 1);
        assert_eq!(body(&m).len(), 3);
        assert!(matches!(
            m.funcs[0].blocks[0].term,
            Terminator::Ret(Some(Operand::Value(ValueId(2))))
        ));
    }

    #[test]
    fn ashr_propagates_sext32_fact() {
        // %3 = ashr(sx(%0), 5) is sext-32, so a fresh pair over it
        // elides.
        let mut vals = vec![];
        let insts = vec![
            val(0, binop(BinOp::Add, c(7), c(9)), &mut vals),
            val(1, binop(BinOp::Shl, v(0), c(32)), &mut vals),
            val(2, binop(BinOp::AShr, v(1), c(32)), &mut vals),
            val(3, binop(BinOp::AShr, v(2), c(5)), &mut vals),
            val(4, binop(BinOp::Shl, v(3), c(32)), &mut vals),
            val(5, binop(BinOp::AShr, v(4), c(32)), &mut vals),
        ];
        let mut m = module_one_block(insts, vals, Terminator::Ret(Some(v(5))));
        let stats = elide_sext_pairs(&mut m, &[]);
        assert_eq!(stats.pairs_elided, 1);
        assert!(matches!(
            m.funcs[0].blocks[0].term,
            Terminator::Ret(Some(Operand::Value(ValueId(3))))
        ));
    }

    #[test]
    fn transposed_result_feeds_downstream_elide() {
        // (a & b) chained into another normalization: the transposed
        // and's result is a pair (sext-32), so the chain's fresh pair
        // over it elides — R1 and R2 compose in one walk.
        let mut vals = vec![];
        let insts = vec![
            val(0, binop(BinOp::Add, c(7), c(9)), &mut vals),
            val(1, binop(BinOp::Sub, v(0), c(1)), &mut vals),
            val(2, binop(BinOp::Shl, v(0), c(32)), &mut vals),
            val(3, binop(BinOp::AShr, v(2), c(32)), &mut vals),
            val(4, binop(BinOp::Shl, v(1), c(32)), &mut vals),
            val(5, binop(BinOp::AShr, v(4), c(32)), &mut vals),
            val(6, binop(BinOp::And, v(3), v(5)), &mut vals),
            val(7, binop(BinOp::Shl, v(6), c(32)), &mut vals),
            val(8, binop(BinOp::AShr, v(7), c(32)), &mut vals),
        ];
        let mut m = module_one_block(insts, vals, Terminator::Ret(Some(v(8))));
        let stats = elide_sext_pairs(&mut m, &[]);
        assert_eq!(stats.transposed, 1);
        assert_eq!(stats.pairs_elided, 1);
        // add, sub, and, shl, ashr(%6) — %7/%8's pair gone, ret %6.
        assert_eq!(body(&m).len(), 5);
        assert!(matches!(
            m.funcs[0].blocks[0].term,
            Terminator::Ret(Some(Operand::Value(ValueId(6))))
        ));
    }

    #[test]
    fn multi_def_copy_stays_unknown() {
        // %1 has two Copy defs (post-φ loop cell) — a pair over it
        // must survive: no fixpoint, loop-carried values are unknown.
        let vals = vec![
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
        ];
        let f = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        Inst {
                            result: Some(ValueId(0)),
                            kind: binop(BinOp::Add, c(1), c(2)),
                            origin: None,
                        },
                        Inst {
                            result: Some(ValueId(1)),
                            kind: InstKind::Copy(Type::I64, v(0)),
                            origin: None,
                        },
                    ],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![
                        Inst {
                            result: Some(ValueId(2)),
                            kind: binop(BinOp::Shl, v(1), c(32)),
                            origin: None,
                        },
                        Inst {
                            result: Some(ValueId(3)),
                            kind: binop(BinOp::AShr, v(2), c(32)),
                            origin: None,
                        },
                        Inst {
                            result: Some(ValueId(1)),
                            kind: InstKind::Copy(Type::I64, v(3)),
                            origin: None,
                        },
                    ],
                    term: Terminator::Ret(Some(v(3))),
                },
            ],
            values: vals,
            current_origin: None,
        };
        let mut m = Module {
            funcs: vec![f],
            ..Default::default()
        };
        let stats = elide_sext_pairs(&mut m, &[]);
        assert_eq!(stats, SextElideStats::default());
        assert_eq!(m.funcs[0].blocks[1].insts.len(), 3);
    }

    #[test]
    fn seeded_multi_def_cell_elides_pair() {
        // same shape as multi_def_copy_stays_unknown, but the interval
        // analysis certified the cell — its pair collapses.
        let vals = vec![
            ValueInfo {
                ty: Type::I64,
                name: None,
            };
            4
        ];
        let f = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        Inst {
                            result: Some(ValueId(0)),
                            kind: binop(BinOp::Add, c(1), c(2)),
                            origin: None,
                        },
                        Inst {
                            result: Some(ValueId(1)),
                            kind: InstKind::Copy(Type::I64, v(0)),
                            origin: None,
                        },
                    ],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![
                        Inst {
                            result: Some(ValueId(2)),
                            kind: binop(BinOp::Shl, v(1), c(32)),
                            origin: None,
                        },
                        Inst {
                            result: Some(ValueId(3)),
                            kind: binop(BinOp::AShr, v(2), c(32)),
                            origin: None,
                        },
                        Inst {
                            result: Some(ValueId(1)),
                            kind: InstKind::Copy(Type::I64, v(3)),
                            origin: None,
                        },
                    ],
                    term: Terminator::Ret(Some(v(3))),
                },
            ],
            values: vals,
            current_origin: None,
        };
        let mut m = Module {
            funcs: vec![f],
            ..Default::default()
        };
        let seed: HashSet<ValueId> = [ValueId(1)].into_iter().collect();
        let stats = elide_sext_pairs(&mut m, &[seed]);
        assert_eq!(stats.pairs_elided, 1);
        assert_eq!(stats.dead_shifts_removed, 1);
        // the pair is gone; the cell's loop def now copies the cell's
        // own source directly and the ret follows the replacement.
        assert_eq!(m.funcs[0].blocks[1].insts.len(), 1);
        assert!(matches!(
            m.funcs[0].blocks[1].insts[0].kind,
            InstKind::Copy(Type::I64, Operand::Value(ValueId(1)))
        ));
        assert!(matches!(
            m.funcs[0].blocks[1].term,
            Terminator::Ret(Some(Operand::Value(ValueId(1))))
        ));
    }
}
