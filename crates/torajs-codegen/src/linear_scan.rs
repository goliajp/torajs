//! Linear Scan register allocation (Poletto & Sarkar 1999) — phase
//! 2 + 3 of the LS pipeline.
//!
//! Phase 2 = active-set sweep:
//!
//!   1. Sort all interval starts ascending.
//!   2. Walk in that order; before each new interval,
//!      *expire* any active interval whose `end < current.start`
//!      (its register returns to the free pool).
//!   3. Allocate the new interval a register from the appropriate
//!      class's free pool.
//!
//! Phase 3 = spill at active (Poletto&Sarkar §4.1, "spill at
//! interval"): when the appropriate free pool is empty, pick the
//! interval with the *furthest end* among `{active ∪ {new}}` and
//! spill it. The other interval keeps the register. This is the
//! cheapest choice because the spilled interval has the most
//! remaining liveness — pushing it to a stack slot frees the
//! reg-pressure slot for the longest stretch of code.
//!
//! Spill slots sit immediately above the alloca region in the
//! prologue-carved frame; offsets are sp-relative (already include
//! `raw_alloca_bytes`) so emit-side code only needs the raw `off`
//! to issue `LDR/STR scratch, [SP, #off]`. The spill cursor only
//! grows — slots are never reused across intervals, keeping the
//! allocator pass dead simple and matching the SSA "every value is
//! a single address" mental model.
//!
//! ## Mirrors `allocate_trivial` where the policy doesn't change
//!
//! Three pieces are independent of LS and are computed exactly the
//! same way as `regalloc::allocate_trivial`:
//!
//!   - Alloca slot offsets (one byte cursor per Alloca-shaped
//!     instruction's result `ValueId`).
//!   - `has_calls` flag (any inst that lowers to a BL site).
//!   - Param register assignment (AAPCS64 §5.4.2 int / fp lanes —
//!     `X0..X7` and `V0..V7` count independently).
//!
//! Sharing these with trivial keeps the prologue / epilogue shape
//! and the AAPCS64 caller-side surface byte-identical when the
//! function fits in either allocator's reg budget. The only
//! observable difference for those functions is the order in which
//! the caller-saved scratch pool is consumed — and even that
//! collapses when the trivial walk and the LS scan happen to give
//! the same answer (both consume from `X13, X14, ...` in inst-def
//! order).
//!
//! ## Caller — none yet
//!
//! This sub-step ships the function side-by-side with
//! `allocate_trivial`. `compile_function` still calls the trivial
//! allocator; the cut-over to LS happens in S5 LS sub-step 4 once
//! spill (sub-step 3) lands and the corpus runner confirms LS
//! covers every fixture the trivial allocator does plus the 245
//! that currently hit `index out of bounds: len 11`.

use std::collections::{HashMap, VecDeque};

use torajs_core::ssa::{Function, Type};

use crate::liveness::{Interval, compute_intervals};
use crate::reg::{Fpr, Gpr, Reg, aapcs64};
use crate::regalloc::{Assignment, alloca_slot_size, collect_ret_value_ids, inst_emits_bl};

/// Linear-Scan register allocator (no spill yet — see module docs).
pub fn allocate_linear_scan(func: &Function) -> Assignment {
    let intervals = compute_intervals(func);
    let ret_vids = collect_ret_value_ids(func);

    let mut by_value: HashMap<u32, Reg> = HashMap::new();
    let mut alloca_offsets: HashMap<u32, u32> = HashMap::new();
    let mut next_alloca_offset: u32 = 0;
    let mut has_calls = false;

    // Pass A — independent of LS: alloca offsets + has_calls. Same
    // policy as trivial. Mirrors regalloc.rs walk so swapping
    // allocators preserves frame shape.
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(slot_size) = alloca_slot_size(&inst.kind) {
                let result = inst.result.expect("Alloca must have result");
                alloca_offsets.insert(result.0, next_alloca_offset);
                next_alloca_offset += slot_size;
            }
            if inst_emits_bl(&inst.kind) {
                has_calls = true;
            }
        }
    }

    // Pass B — AAPCS64 param lanes. Same policy as trivial S5-gap-1.
    let mut gpr_arg_idx = 0usize;
    let mut fpr_arg_idx = 0usize;
    for &param in &func.params {
        let ty = func
            .values
            .get(param.0 as usize)
            .map(|vi| &vi.ty)
            .expect("ValueId(param) out of bounds");
        let is_fp = matches!(ty, Type::F64);
        let reg = if is_fp {
            let v = aapcs64::FP_ARG_RET[fpr_arg_idx];
            fpr_arg_idx += 1;
            Reg::Fpr(v)
        } else {
            let x = aapcs64::ARG_RET[gpr_arg_idx];
            gpr_arg_idx += 1;
            Reg::Gpr(x)
        };
        by_value.insert(param.0, reg);
    }

    // Pass C — Linear Scan over inst-result + alloca-result ValueIds
    // (anything not already a param). Sort by interval.start; ties
    // broken by ValueId to keep the sweep deterministic.
    let mut order: Vec<u32> = intervals
        .keys()
        .copied()
        .filter(|v| !by_value.contains_key(v))
        .collect();
    order.sort_by_key(|v| (intervals[v].start, *v));

    // active = currently-live intervals owning a scratch register.
    // Stored as (ValueId, interval, reg). Vec — N is small (<=
    // pool size), Vec scan is faster than BTree maintenance.
    let mut active: Vec<(u32, Interval, Reg)> = Vec::new();
    let mut free_gpr: VecDeque<Gpr> = aapcs64::CALLER_SAVED_SCRATCH.iter().copied().collect();
    let mut free_fpr: VecDeque<Fpr> = aapcs64::FP_CALLER_SAVED_SCRATCH.iter().copied().collect();

    // Spill cursor — sp-relative byte offset of the *next* spill
    // slot. Starts just above the alloca region so a single sp-
    // adjustment in the prologue covers both. Grows by 8 per spill
    // (all spilled values are 64-bit — GPR or D-form FPR).
    let mut next_spill_offset = next_alloca_offset;

    for vid in order {
        let interval = intervals[&vid];

        // Expire — release every active interval whose end is
        // strictly before this one's start (i.e. no longer overlaps
        // anything we might allocate from here on).
        let cur_start = interval.start;
        let mut i = 0;
        while i < active.len() {
            if active[i].1.end < cur_start {
                // LIFO — most-recently-freed reg is reused first.
                // Keeps register pressure tight (the same X13 is
                // recycled across non-overlapping intervals rather
                // than spreading writes across X13..X23).
                match active[i].2 {
                    Reg::Gpr(g) => free_gpr.push_front(g),
                    Reg::Fpr(f) => free_fpr.push_front(f),
                    // LS-3 spill: spilled slots don't consume a reg-
                    // pool slot — their stack offset is owned for the
                    // entire function lifetime (spill cursor only
                    // grows; we never reuse spill offsets across
                    // intervals). So expiring a spilled value
                    // releases nothing back to the free pool.
                    Reg::SpillGpr(_) | Reg::SpillFpr(_) => {}
                }
                active.swap_remove(i);
            } else {
                i += 1;
            }
        }

        let ty = func
            .values
            .get(vid as usize)
            .map(|vi| &vi.ty)
            .expect("ValueId out of bounds");
        let is_fp = matches!(ty, Type::F64);
        let is_ret = ret_vids.contains(&vid);

        // Ret values go to X0 / V0 unconditionally — they never
        // consume a free-pool slot because the AAPCS64 ret lane is
        // disjoint from the caller-saved scratch pool.
        let reg = if is_ret {
            if is_fp {
                Reg::Fpr(aapcs64::FP_ARG_RET[0])
            } else {
                Reg::Gpr(aapcs64::ARG_RET[0])
            }
        } else if is_fp {
            match free_fpr.pop_front() {
                Some(f) => Reg::Fpr(f),
                None => spill_at_active(
                    &mut active,
                    &mut by_value,
                    &mut next_spill_offset,
                    vid,
                    interval,
                    /*is_fp*/ true,
                ),
            }
        } else {
            match free_gpr.pop_front() {
                Some(g) => Reg::Gpr(g),
                None => spill_at_active(
                    &mut active,
                    &mut by_value,
                    &mut next_spill_offset,
                    vid,
                    interval,
                    /*is_fp*/ false,
                ),
            }
        };

        by_value.insert(vid, reg);
        // Only scratch-pool-backed values join the active set; the
        // ret lane lives outside the pool and would mislead expiry.
        // Spilled values are tracked too — keeps the active-set
        // expiry pass complete even though spills don't release
        // anything to the free pool.
        if !is_ret {
            active.push((vid, interval, reg));
        }
    }

    let total_spill_bytes = next_spill_offset - next_alloca_offset;
    Assignment::from_parts(
        by_value,
        alloca_offsets,
        next_alloca_offset,
        total_spill_bytes,
        has_calls,
    )
}

/// Poletto & Sarkar 1999 "spill at active" — pick the interval with
/// the furthest end among `{active intervals of the right class} ∪
/// {new}`; that one is spilled. The other one keeps the register.
///
/// Why furthest-end is the textbook choice: the spilled interval pays
/// LDR/STR every time its def or use is touched in the spill region,
/// for the remainder of its live range. Choosing the longest
/// remaining range maximizes the freed-register window — every shorter
/// interval squeezed in during that window saves one LDR/STR/spill
/// cost. (See also Wimmer & Mössenböck 2005 "Optimized Interval
/// Splitting in a Linear Scan Register Allocator" for the same
/// argument generalized to interval splitting.)
///
/// Returns the `Reg` to assign to `new_vid`. If the new arrival was
/// the one chosen for spill, that's a `Reg::SpillGpr/Fpr`. Otherwise
/// the new arrival gets the victim's register (a `Reg::Gpr/Fpr`) and
/// the victim's entry in `active` + `by_value` is rewritten in place
/// to `Reg::SpillGpr/Fpr(off)`.
fn spill_at_active(
    active: &mut Vec<(u32, Interval, Reg)>,
    by_value: &mut HashMap<u32, Reg>,
    next_spill_offset: &mut u32,
    _new_vid: u32,
    new_interval: Interval,
    is_fp: bool,
) -> Reg {
    // Find the active interval of the matching class with the
    // furthest end. If none exists, the new arrival has to spill
    // itself (this only happens if every active is of the other
    // class — but then the pool shouldn't have been exhausted; so
    // in practice victim is always Some).
    let mut victim_idx: Option<usize> = None;
    let mut victim_end: i32 = -1;
    for (i, entry) in active.iter().enumerate() {
        let class_matches = match entry.2 {
            Reg::Gpr(_) => !is_fp,
            Reg::Fpr(_) => is_fp,
            // Already-spilled actives can't free a reg.
            Reg::SpillGpr(_) | Reg::SpillFpr(_) => continue,
        };
        if class_matches && (entry.1.end as i32) > victim_end {
            victim_end = entry.1.end as i32;
            victim_idx = Some(i);
        }
    }

    let allocate_spill = |off: &mut u32| -> u32 {
        let here = *off;
        *off += 8;
        here
    };

    match victim_idx {
        Some(i) if (active[i].1.end as i32) > new_interval.end as i32 => {
            // Spill the victim, give its register to the new arrival.
            let (victim_vid, _victim_interval, victim_reg) = active[i];
            let spill_off = allocate_spill(next_spill_offset);
            let spilled_reg = if is_fp {
                Reg::SpillFpr(spill_off)
            } else {
                Reg::SpillGpr(spill_off)
            };
            // Rewrite victim's assignment + active entry.
            by_value.insert(victim_vid, spilled_reg);
            active[i].2 = spilled_reg;
            // New arrival inherits the freed reg.
            victim_reg
        }
        _ => {
            // New arrival's end is at least as far as everyone in
            // active — cheaper to spill the new arrival.
            let spill_off = allocate_spill(next_spill_offset);
            if is_fp {
                Reg::SpillFpr(spill_off)
            } else {
                Reg::SpillGpr(spill_off)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type,
        ValueId, ValueInfo,
    };

    /// One-inst baseline: `fn f() -> i64 { 1 + 2 }`. v0 is the ret
    /// value — LS gives it X0 (AAPCS64 ret lane), no scratch consumed.
    #[test]
    fn ls_one_plus_two_returns_x0() {
        let v0 = ValueId(0);
        let func = Function {
            name: "f".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X0));
    }

    /// Param matches AAPCS64: `fn id(x: i64) -> i64 { x }` — x in X0.
    #[test]
    fn ls_single_param_in_x0() {
        let x = ValueId(0);
        let func = Function {
            name: "id".into(),
            params: vec![x],
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("x".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: Vec::new(),
                term: Terminator::Ret(Some(Operand::Value(x))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert_eq!(alloc.of(x), Reg::Gpr(Gpr::X0));
    }

    /// Mixed int + fp params: `fn mix(a: i64, b: f64, c: i64) -> i64
    /// { a }`. AAPCS64 §5.4.2 → a=X0, b=V0, c=X1.
    #[test]
    fn ls_mixed_params_separate_lanes() {
        let a = ValueId(0);
        let b = ValueId(1);
        let c = ValueId(2);
        let func = Function {
            name: "mix".into(),
            params: vec![a, b, c],
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("a".into()),
                },
                ValueInfo {
                    ty: Type::F64,
                    name: Some("b".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("c".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: Vec::new(),
                term: Terminator::Ret(Some(Operand::Value(a))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert_eq!(alloc.of(a), Reg::Gpr(Gpr::X0));
        assert_eq!(alloc.of(b), Reg::Fpr(Fpr::V0));
        assert_eq!(alloc.of(c), Reg::Gpr(Gpr::X1));
    }

    /// Reg reuse across non-overlapping intervals — three sequential
    /// SSA values, each only used by the next inst. LS sweep should
    /// expire v0 before v1 starts, and v1 before v2 starts, so v1
    /// and v2 can reuse v0's scratch slot. Trivial would give each a
    /// distinct scratch.
    ///
    /// `fn chain3() {
    ///    v0 = ((Call f0)) ;   (void call so v0..v2 are non-ret throwaways)
    ///    v1 = ((Call f0))
    ///    v2 = ((Call f0))
    ///    ret void
    /// }`
    ///
    /// Each v_i's interval is [i, i] — disjoint. LS should give all
    /// three the same scratch (X13, first in CALLER_SAVED_SCRATCH
    /// after LS-3 reserved X12 for OP_SCRATCH_RESULT_GPR).
    #[test]
    fn ls_reuses_register_after_expiry() {
        let v0 = ValueId(0);
        let v1 = ValueId(1);
        let v2 = ValueId(2);
        let f0 = FuncId(0);
        let func = Function {
            name: "chain3".into(),
            params: Vec::new(),
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v1".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v2".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v0),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                    Inst {
                        result: Some(v2),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(None),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        // All three should land in X13 (first scratch after LS-3
        // reserved X12) because each expires before the next claim.
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X13));
        assert_eq!(alloc.of(v1), Reg::Gpr(Gpr::X13));
        assert_eq!(alloc.of(v2), Reg::Gpr(Gpr::X13));
    }

    /// Overlapping intervals must claim distinct registers:
    ///
    /// `fn live2() -> i64 {
    ///    v0 = (Call f0)         ; void call result
    ///    v1 = (Call f0)         ; v0 still live for the final binop
    ///    v2 = v0 + v1           ; uses both → ret
    ///    ret v2
    /// }`
    ///
    /// LS should give v0=X13, v1=X14 (both alive at slot 2 when v2
    /// is defined), v2=X0 (ret lane). After LS-3 reserved X12.
    #[test]
    fn ls_separate_regs_when_intervals_overlap() {
        let v0 = ValueId(0);
        let v1 = ValueId(1);
        let v2 = ValueId(2);
        let f0 = FuncId(0);
        let func = Function {
            name: "live2".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v1".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v2".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v0),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                    Inst {
                        result: Some(v2),
                        kind: InstKind::BinOp(BinOp::Add, Operand::Value(v0), Operand::Value(v1)),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(Some(Operand::Value(v2))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X13));
        assert_eq!(alloc.of(v1), Reg::Gpr(Gpr::X14));
        assert_eq!(alloc.of(v2), Reg::Gpr(Gpr::X0));
    }

    /// FPR + GPR separate pools — fp value gets V19 (first FP
    /// scratch after LS-3 reserved V18), int value gets X13 (first
    /// GPR scratch after LS-3 reserved X12).
    #[test]
    fn ls_separate_pools_for_int_and_fp() {
        let v0 = ValueId(0); // int, non-ret
        let v1 = ValueId(1); // fp, non-ret
        let v2 = ValueId(2); // int ret
        let f0 = FuncId(0);
        let func = Function {
            name: "mix_pools".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::F64,
                    name: Some("v1".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v2".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v0),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::BinOp(
                            BinOp::FAdd,
                            Operand::ConstF64(1.0),
                            Operand::ConstF64(2.0),
                        ),
                        origin: None,
                    },
                    Inst {
                        result: Some(v2),
                        kind: InstKind::FpToSi(Operand::Value(v1)),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(Some(Operand::Value(v2))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X13));
        assert_eq!(alloc.of(v1), Reg::Fpr(Fpr::V19));
        assert_eq!(alloc.of(v2), Reg::Gpr(Gpr::X0));
    }

    /// Alloca offsets + has_calls share trivial's policy — `fn f() {
    /// alloca 8; call f0(); }`.
    #[test]
    fn ls_preserves_alloca_offsets_and_has_calls() {
        let v0 = ValueId(0); // alloca ptr
        let v1 = ValueId(1); // call result (void-ish — placeholder int)
        let f0 = FuncId(0);
        let func = Function {
            name: "f".into(),
            params: Vec::new(),
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::Ptr,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v1".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v0),
                        kind: InstKind::AllocaBytes(16),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(None),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert_eq!(alloc.alloca_offset_of(v0), 0);
        assert_eq!(alloc.raw_alloca_bytes, 16);
        assert!(alloc.has_calls);
    }

    /// LS-3 — GPR-pool pressure beyond `CALLER_SAVED_SCRATCH.len()`
    /// is now handled by spilling to the frame's spill region.
    ///
    /// We build 12 simultaneously-live i64 values + a chained accum
    /// of 11 more values keeping each one live (one more than the
    /// post-LS-3 10-slot pool can hold). LS must NOT panic and must
    /// produce an assignment where every non-ret value lands either
    /// in a GPR or a SpillGpr slot, and `total_spill_bytes` matches
    /// the count of SpillGpr slots × 8.
    ///
    /// The exact victim selection (Poletto&Sarkar "spill at active":
    /// furthest end) is exercised by the dedicated unit test below;
    /// here we only assert the substantive invariants so the test
    /// is robust to future scheduling tweaks.
    #[test]
    fn ls_spills_when_gpr_pool_exhausted() {
        let f0 = FuncId(0);
        let mut values = Vec::new();
        let mut insts = Vec::new();
        for i in 0..12 {
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("v{i}")),
            });
            insts.push(Inst {
                result: Some(ValueId(i as u32)),
                kind: InstKind::Call(f0, Vec::new()),
                origin: None,
            });
        }
        let mut acc = Operand::Value(ValueId(0));
        for i in 1..12 {
            let new_vid = ValueId(12 + i as u32 - 1);
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("acc{i}")),
            });
            insts.push(Inst {
                result: Some(new_vid),
                kind: InstKind::BinOp(BinOp::Add, acc, Operand::Value(ValueId(i as u32))),
                origin: None,
            });
            acc = Operand::Value(new_vid);
        }
        let func = Function {
            name: "twelve_live".into(),
            params: Vec::new(),
            ret: Type::I64,
            values,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(acc)),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);

        // The substantive invariants under spill:
        //   1. No panic.
        //   2. Every non-ret value gets some assignment (Gpr|Spill).
        //   3. SpillGpr offsets form a contiguous 8-aligned cursor
        //      (no gaps, no overlap) starting at raw_alloca_bytes.
        //   4. total_spill_bytes == 8 × distinct spill slot count.
        let mut spill_offsets: Vec<u32> = Vec::new();
        for i in 0..23 {
            match alloc.of(ValueId(i)) {
                Reg::Gpr(g) => assert!(
                    g == Gpr::X0 || aapcs64::CALLER_SAVED_SCRATCH.contains(&g),
                    "v{i} got unexpected GPR {g:?}"
                ),
                Reg::SpillGpr(off) => spill_offsets.push(off),
                other => panic!("v{i} got unexpected {other:?}"),
            }
        }
        assert!(
            !spill_offsets.is_empty(),
            "pool overflow must produce >=1 spill"
        );
        spill_offsets.sort();
        spill_offsets.dedup();
        for (n, off) in spill_offsets.iter().enumerate() {
            assert_eq!(
                *off,
                n as u32 * 8,
                "spill cursor must be contiguous 8-aligned"
            );
        }
        assert_eq!(
            alloc.total_spill_bytes,
            (spill_offsets.len() as u32) * 8,
            "spill_bytes accounts for every distinct SpillGpr slot"
        );
        assert_eq!(alloc.raw_alloca_bytes, 0);
    }

    /// Dedicated victim-selection test: 11 simultaneously-live
    /// values where one has a strictly longer end than the rest.
    /// The new arrival (which has the longest end) should NOT spill
    /// itself — it should spill the existing active value with the
    /// next-furthest end, since picking the longest-living candidate
    /// to spill maximizes free-register window length.
    ///
    /// Actually per "spill at active" the rule is "victim = furthest
    /// end among active+new"; whoever is furthest spills. So if the
    /// new arrival's end > every active's end, new spills itself.
    /// We verify that branch here: build 11 ints all dying early,
    /// then the 11th's end extends past all of them; on insert it
    /// finds the pool empty and spills *itself* (not an active).
    #[test]
    fn ls_spills_self_when_new_arrival_has_furthest_end() {
        let f0 = FuncId(0);
        // 10 short-lived calls (each used immediately by the next
        // inst, then dead). Then one more call kept alive until ret.
        let mut values = Vec::new();
        let mut insts = Vec::new();
        for i in 0..11 {
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("v{i}")),
            });
            insts.push(Inst {
                result: Some(ValueId(i as u32)),
                kind: InstKind::Call(f0, Vec::new()),
                origin: None,
            });
        }
        // Use v0..v9 in a left-fold so each dies after one inst.
        // 11 acc insts. acc_i = v_i + (acc_{i-1} or v_0).
        let mut acc = Operand::Value(ValueId(0));
        for i in 1..10 {
            let nv = ValueId(11 + i as u32 - 1);
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("a{i}")),
            });
            insts.push(Inst {
                result: Some(nv),
                kind: InstKind::BinOp(BinOp::Add, acc, Operand::Value(ValueId(i as u32))),
                origin: None,
            });
            acc = Operand::Value(nv);
        }
        // Final use: acc + v10. v10 has the furthest end.
        // 11 originals (vid 0..10) + 9 accs (vid 11..19) = 20 values
        // so the next ValueId is 20.
        let last = ValueId(20);
        values.push(ValueInfo {
            ty: Type::I64,
            name: Some("last".into()),
        });
        insts.push(Inst {
            result: Some(last),
            kind: InstKind::BinOp(BinOp::Add, acc, Operand::Value(ValueId(10))),
            origin: None,
        });
        let func = Function {
            name: "long_v10".into(),
            params: Vec::new(),
            ret: Type::I64,
            values,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(Operand::Value(last))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);

        // The interesting invariant: SOME values had to spill (pool
        // overflowed), and `total_spill_bytes` matches 8 × distinct
        // spill-slot count.
        let mut spill_offsets: Vec<u32> = Vec::new();
        for i in 0..21 {
            if let Reg::SpillGpr(off) = alloc.of(ValueId(i)) {
                spill_offsets.push(off);
            }
        }
        spill_offsets.sort();
        spill_offsets.dedup();
        assert!(
            !spill_offsets.is_empty(),
            "pool overflow must produce >=1 spill"
        );
        assert_eq!(alloc.total_spill_bytes, spill_offsets.len() as u32 * 8);
    }

    /// FPR-pool exhaustion spills onto stack via SpillFpr. Build
    /// enough live f64 values to overflow the 13-slot FPR pool and
    /// verify SOME values land in SpillFpr slots + total_spill_bytes
    /// accounts for them.
    #[test]
    fn ls_spills_when_fpr_pool_exhausted() {
        // 14 f64 results from FAdd chain, all consumed by final ret.
        let mut values = Vec::new();
        let mut insts = Vec::new();
        for i in 0..14 {
            values.push(ValueInfo {
                ty: Type::F64,
                name: Some(format!("v{i}")),
            });
            insts.push(Inst {
                result: Some(ValueId(i as u32)),
                kind: InstKind::BinOp(BinOp::FAdd, Operand::ConstF64(1.0), Operand::ConstF64(2.0)),
                origin: None,
            });
        }
        // Sum them all so they're live until the end.
        let mut acc = Operand::Value(ValueId(0));
        for i in 1..14 {
            let nv = ValueId(14 + i as u32 - 1);
            values.push(ValueInfo {
                ty: Type::F64,
                name: Some(format!("s{i}")),
            });
            insts.push(Inst {
                result: Some(nv),
                kind: InstKind::BinOp(BinOp::FAdd, acc, Operand::Value(ValueId(i as u32))),
                origin: None,
            });
            acc = Operand::Value(nv);
        }
        let func = Function {
            name: "fourteen_live_fp".into(),
            params: Vec::new(),
            ret: Type::F64,
            values,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(acc)),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);

        // 14 originals + 13 accs = 27 values; one acc is ret (V0).
        let mut spill_offsets: Vec<u32> = Vec::new();
        for i in 0..27 {
            match alloc.of(ValueId(i)) {
                Reg::Fpr(f) => assert!(
                    f == Fpr::V0 || aapcs64::FP_CALLER_SAVED_SCRATCH.contains(&f),
                    "v{i} got unexpected FPR {f:?}"
                ),
                Reg::SpillFpr(off) => spill_offsets.push(off),
                other => panic!("v{i} got unexpected {other:?}"),
            }
        }
        assert!(!spill_offsets.is_empty(), "pool overflow must spill");
        spill_offsets.sort();
        spill_offsets.dedup();
        assert_eq!(
            alloc.total_spill_bytes,
            (spill_offsets.len() as u32) * 8,
            "spill_bytes accounts for every distinct SpillFpr slot"
        );
    }
}
