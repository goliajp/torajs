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

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Type};

use crate::linear_scan_lanes::{param_lanes, ret_lane_free};
use crate::linear_scan_sweep::Sweep;
use crate::liveness::{
    Interval, compute_intervals, visit_inst_operands, visit_terminator_operands,
};
use crate::reg::{Reg, aapcs64};
use crate::regalloc::{Assignment, alloca_slot_size, collect_ret_value_ids, inst_emits_bl};
use crate::spill_weight::compute_spill_weights;

/// `true` iff some call site falls strictly inside `[start, end]` —
/// the value is defined before a `BL` and still used after it, so a
/// caller-saved register would be clobbered. `p == start` means the
/// value IS the call's result (written after the call returns, safe);
/// `p == end` means it's consumed as that call's argument and dies
/// immediately (safe). The strict `<` on both ends excludes those.
/// ValueIds whose linear interval does not bound their life: a value
/// read at some slot and written at a LATER one.
///
/// The SSA the lowerer produces is single-assignment and never does
/// this, but φ destruction (`phi_promote::destruct`) is not: it gives
/// one ValueId a `copy` in every predecessor of a join, and a join can
/// be numbered before its predecessors. The interval then runs from
/// the use to the last def, so a call sitting in the join AHEAD of the
/// use falls outside `start < p < end` by construction — no linear
/// range describes a life that runs backwards through block order.
///
/// Only that shape qualifies. A value written more than once with
/// every write ahead of every read still has a sound interval and is
/// left alone — asking for callee-saved there costs registers and buys
/// nothing (measured: a 5% loss on one bench case when this set was
/// merely "multi-def").
///
/// The slot numbering must match [`compute_intervals`]: one index per
/// inst, then one for the terminator.
fn collect_unbounded_values(func: &Function) -> HashSet<u32> {
    let mut first_use: HashMap<u32, u32> = HashMap::new();
    let mut last_def: HashMap<u32, u32> = HashMap::new();
    let mut idx: u32 = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            visit_inst_operands(&inst.kind, |v| {
                first_use.entry(v.0).or_insert(idx);
            });
            if let Some(r) = inst.result {
                last_def.insert(r.0, idx);
            }
            idx += 1;
        }
        visit_terminator_operands(&block.term, |v| {
            first_use.entry(v.0).or_insert(idx);
        });
        idx += 1;
    }
    last_def
        .iter()
        .filter(|(v, d)| first_use.get(*v).is_some_and(|u| u < *d))
        .map(|(v, _)| *v)
        .collect()
}

fn crosses_call(interval: Interval, call_sites: &[u32]) -> bool {
    call_sites
        .iter()
        .any(|&p| interval.start < p && p < interval.end)
}

/// Outgoing stack-arg area size — when any call site passes more than
/// 8 args of a register class, the overflow goes to stack slots based
/// at the call-moment SP. The area lives at the BOTTOM of the frame
/// (sp+0), so every alloca / spill offset is biased past it;
/// `args.len() - 8` upper-bounds the per-class overflow count (int +
/// fp lanes together absorb at least 8 args), trading at most a few
/// slack slots for not needing callee sigs here.
fn outgoing_arg_bytes(func: &Function) -> u32 {
    let mut outgoing_slots: u32 = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::Call(_, args) | InstKind::CallIndirect(_, _, args) = &inst.kind {
                outgoing_slots = outgoing_slots.max(args.len().saturating_sub(8) as u32);
            }
        }
    }
    (outgoing_slots * 8 + 15) & !15
}

/// Linear-Scan register allocator with call-crossing awareness and
/// stack spill (see module docs).
pub fn allocate_linear_scan(func: &Function) -> Assignment {
    let intervals = compute_intervals(func);
    let ret_vids = collect_ret_value_ids(func);
    let unbounded = collect_unbounded_values(func);

    // Pass A — alloca offsets, has_calls, and the global inst-slot
    // index of every call site. The numbering MUST match
    // `compute_intervals` (each block contributes one index per inst
    // plus one for its terminator) so a call site `p` compares
    // directly against interval `[start, end]` bounds.
    let outgoing_bytes = outgoing_arg_bytes(func);

    let mut alloca_offsets: HashMap<u32, u32> = HashMap::new();
    let mut next_alloca_offset: u32 = outgoing_bytes;
    let mut has_calls = false;
    let mut call_sites: Vec<u32> = Vec::new();
    let mut idx: u32 = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(slot_size) = alloca_slot_size(&inst.kind) {
                let result = inst.result.expect("Alloca must have result");
                alloca_offsets.insert(result.0, next_alloca_offset);
                next_alloca_offset += slot_size;
            }
            if inst_emits_bl(&inst.kind) {
                has_calls = true;
                call_sites.push(idx);
            }
            idx += 1;
        }
        idx += 1; // terminator slot
    }

    let (param_arg_reg, stack_params) = param_lanes(func);

    let mut sweep = Sweep::new(next_alloca_offset, compute_spill_weights(func));

    // Sweep every interval (params + inst/alloca results) by start;
    // ties broken by ValueId to keep the sweep deterministic.
    let mut order: Vec<u32> = intervals.keys().copied().collect();
    order.sort_by_key(|v| (intervals[v].start, *v));

    for vid in order {
        let interval = intervals[&vid];
        sweep.expire(interval.start);

        let ty = func
            .values
            .get(vid as usize)
            .map(|vi| &vi.ty)
            .expect("ValueId out of bounds");
        let is_fp = matches!(ty, Type::F64);
        // Params def at function ENTRY — before inst slot 0 — so a
        // call at p == interval.start still clobbers their
        // caller-saved arg register. `crosses_call`'s `p == start`
        // exemption is only sound for the call's own result; for a
        // param it silently hands out a clobbered arg reg (post-
        // inline/slot-forward SSA exposed this: a param consumed
        // after a leading call read garbage from x1).
        let crossing = if param_arg_reg.contains_key(&vid) || stack_params.contains_key(&vid) {
            call_sites
                .iter()
                .any(|&p| interval.start <= p && p < interval.end)
        } else if unbounded.contains(&vid) {
            // See `collect_unbounded_values`: the interval does not
            // bound this value's life, so treat it as crossing
            // whenever the function calls anything at all.
            has_calls
        } else {
            crosses_call(interval, &call_sites)
        };

        // Stack params (9th+ of a register class) arrive in the
        // caller's outgoing area; the prologue tail copies each into
        // whatever home the sweep picks here (`stack_param_entry_loads`
        // below), so the body treats them like any other value.
        if stack_params.contains_key(&vid) {
            let dst = if crossing {
                sweep.alloc_crossing(vid, interval, is_fp)
            } else {
                sweep.alloc_noncrossing(vid, interval, is_fp)
            };
            sweep.by_value.insert(vid, dst);
            sweep.active.push((vid, interval, dst));
            continue;
        }

        // Params first (they own a fixed AAPCS64 arg register on
        // entry, even when also the ret value — matching the prior
        // Pass-B-then-Pass-C ordering).
        if let Some(&arg_reg) = param_arg_reg.get(&vid) {
            if !crossing {
                // Stays in its arg register; arg regs aren't in any
                // scratch pool, so no active-set entry.
                sweep.by_value.insert(vid, arg_reg);
                continue;
            }
            // Crosses a call — the caller-saved arg register would be
            // clobbered. Relocate to a callee-saved reg (or spill);
            // entry moves are derived from final `by_value` after the
            // sweep so a later `spill_at_active` eviction stays sound.
            let dst = sweep.alloc_crossing(vid, interval, is_fp);
            sweep.by_value.insert(vid, dst);
            sweep.active.push((vid, interval, dst));
            continue;
        }

        // Ret values: when their interval doesn't cross a call, X0/V0
        // is the ABI sink — we park them there directly and
        // `emit_terminator::Ret` is a no-op move at the actual ret.
        //
        // When the interval DOES cross a call (e.g. `return n + "x" +
        // "y"`, where the outer `__torajs_str_concat`'s result lives
        // across a subsequent `__torajs_str_drop` BL before reaching
        // the ret), X0/V0 is caller-saved and the intervening BL
        // clobbers it — silently destroying the ret value. Allocate
        // callee-saved instead; `emit_terminator::Ret` materializes
        // the value into X0/V0 at the actual ret point (its
        // idempotent `if src != X0/V0 { mov }` covers this).
        if ret_vids.contains(&vid) {
            if crossing {
                let dst = sweep.alloc_crossing(vid, interval, is_fp);
                sweep.by_value.insert(vid, dst);
                sweep.active.push((vid, interval, dst));
                continue;
            }
            let reg = if is_fp {
                Reg::Fpr(aapcs64::FP_ARG_RET[0])
            } else {
                Reg::Gpr(aapcs64::ARG_RET[0])
            };
            // That lane belongs to the first param too — see
            // `ret_lane_free` for why the sweep can't see the clash.
            if ret_lane_free(func, reg, interval, &intervals, &sweep.by_value) {
                sweep.by_value.insert(vid, reg);
                continue;
            }
        }

        let reg = if crossing {
            sweep.alloc_crossing(vid, interval, is_fp)
        } else {
            sweep.alloc_noncrossing(vid, interval, is_fp)
        };
        sweep.by_value.insert(vid, reg);
        sweep.active.push((vid, interval, reg));
    }

    let total_spill_bytes = sweep.next_spill_offset - next_alloca_offset;
    let used_callee_gpr_mask = sweep.used_callee_gpr_mask;
    let used_callee_fpr_mask = sweep.used_callee_fpr_mask;

    // Derive entry moves from post-sweep `by_value` — a param the
    // sweep parked in a callee-saved reg may later be evicted to a
    // spill slot by `spill_at_active`; the original push would still
    // `mov` to the reassigned reg while load sites read from the
    // never-written spill slot. Walking final state gives the correct
    // `(arg_reg → callee_reg | SpillGpr)` shape regardless of victim.
    let mut param_entry_moves: Vec<(Reg, Reg)> = Vec::new();
    let mut stack_param_entry_loads: Vec<(u32, Reg)> = Vec::new();
    for &param in &func.params {
        let vid = param.0;
        let final_reg = *sweep
            .by_value
            .get(&vid)
            .expect("param must have a final assignment after sweep");
        if let Some(&slot) = stack_params.get(&vid) {
            stack_param_entry_loads.push((slot, final_reg));
            continue;
        }
        let arg_reg = *param_arg_reg
            .get(&vid)
            .expect("param must have an AAPCS64 arg register");
        if final_reg != arg_reg {
            param_entry_moves.push((arg_reg, final_reg));
        }
    }
    // Deterministic emit order (walked off func.params, but keep the
    // slot order explicit for the reader + any future map-driven walk).
    stack_param_entry_loads.sort_by_key(|&(slot, _)| slot);

    let mut assignment = Assignment::from_parts(
        sweep.by_value,
        alloca_offsets,
        next_alloca_offset,
        total_spill_bytes,
        has_calls,
    );
    assignment.used_callee_gpr_mask = used_callee_gpr_mask;
    assignment.used_callee_fpr_mask = used_callee_fpr_mask;
    assignment.param_entry_moves = param_entry_moves;
    assignment.stack_param_entry_loads = stack_param_entry_loads;
    assignment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::{Fpr, Gpr};
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

    /// Stack params: `fn nine(a..i) -> i64 { i }` — the 9th int param
    /// has no ARG_RET lane; the allocator hands it a home and records
    /// an entry load from incoming slot 0.
    #[test]
    fn ls_ninth_param_gets_stack_entry_load() {
        let vals: Vec<ValueInfo> = (0..9)
            .map(|i| ValueInfo {
                ty: Type::I64,
                name: Some(format!("p{i}")),
            })
            .collect();
        let params: Vec<ValueId> = (0..9).map(ValueId).collect();
        let func = Function {
            name: "nine".into(),
            params,
            ret: Type::I64,
            values: vals,
            blocks: vec![Block {
                id: BlockId(0),
                insts: Vec::new(),
                term: Terminator::Ret(Some(Operand::Value(ValueId(8)))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert_eq!(alloc.stack_param_entry_loads.len(), 1);
        let (slot, dst) = alloc.stack_param_entry_loads[0];
        assert_eq!(slot, 0);
        assert_eq!(alloc.of(ValueId(8)), dst);
        // first 8 params keep their AAPCS64 lanes
        assert_eq!(alloc.of(ValueId(0)), Reg::Gpr(Gpr::X0));
        assert_eq!(alloc.of(ValueId(7)), Reg::Gpr(Gpr::X7));
    }

    /// Outgoing area: a call passing 9 args biases the alloca/spill
    /// region past one 16-byte-aligned outgoing slot chunk.
    #[test]
    fn ls_nine_arg_call_reserves_outgoing_area() {
        let args: Vec<Operand> = (1..=9).map(Operand::ConstI64).collect();
        let func = Function {
            name: "caller".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("r".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(ValueId(0)),
                    kind: InstKind::Call(FuncId(0), args),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        // one overflow slot, 16-aligned → 16 bytes of outgoing area
        // baked into the frame below every alloca/spill offset.
        assert!(alloc.raw_alloca_bytes >= 16);
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
    /// v0 lives across the v1 call (reused at the final binop) so it
    /// must land in a callee-saved register (X19); v1 is that call's
    /// own result and dies right after, so it gets a caller-saved
    /// register (X13). v2 = ret → X0.
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
        // v0 crosses the v1 call (its value is reused at the post-call
        // add) → callee-saved X19; v1 is that call's own result and
        // dies at the add with no intervening call → caller-saved X13;
        // v2 is the ret value → X0.
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X19));
        assert_eq!(alloc.of(v1), Reg::Gpr(Gpr::X13));
        assert_eq!(alloc.of(v2), Reg::Gpr(Gpr::X0));
        assert_eq!(alloc.used_callee_gpr_mask, 1 << Gpr::X19.idx());
    }

    /// Param consumed AFTER a call at inst slot 0 (the post-inline /
    /// slot-forward closure shape: `%arr = call alloc; store %param,
    /// %arr +24`). The param's interval is [0, 2] with a call at
    /// p == 0 == start — `crosses_call`'s start exemption must NOT
    /// apply to params (they def at entry, before the call), so the
    /// param must be relocated off its clobbered arg register with a
    /// matching entry move.
    #[test]
    fn ls_param_crossing_leading_call_relocated() {
        let p0 = ValueId(0); // param, read back after the call
        let v1 = ValueId(1); // the call's own result
        let f0 = FuncId(0);
        let func = Function {
            name: "t".into(),
            params: vec![p0],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v1),
                        kind: InstKind::Call(f0, Vec::new()),
                        origin: None,
                    },
                    Inst {
                        result: None,
                        kind: InstKind::Store(Operand::Value(p0), Operand::Value(v1), 24),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(None),
            }],
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None,
                },
                ValueInfo {
                    ty: Type::Ptr,
                    name: None,
                },
            ],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        let preg = alloc.of(p0);
        assert_ne!(
            preg,
            Reg::Gpr(Gpr::X0),
            "param crossing a leading call must leave its arg register"
        );
        assert!(
            alloc
                .param_entry_moves
                .iter()
                .any(|&(from, to)| from == Reg::Gpr(Gpr::X0) && to == preg),
            "entry move from X0 to the relocated register must exist"
        );
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
                    g == Gpr::X0
                        || aapcs64::CALLER_SAVED_SCRATCH.contains(&g)
                        || aapcs64::CALLEE_SAVED_SCRATCH.contains(&g),
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

    /// Call-crossing values that overflow the 10-slot callee-saved
    /// pool spill to the stack via the "spill at active" path. We
    /// build 14 call results all kept live by a final reduce chain —
    /// every result crosses the later calls, so all of v0..v12 are
    /// call-crossing and only 10 fit in the callee-saved pool; the
    /// rest spill. Asserts the spill cursor is contiguous + accounted
    /// and that the callee-saved pool was actually exercised.
    #[test]
    fn ls_spills_self_when_new_arrival_has_furthest_end() {
        let f0 = FuncId(0);
        let n: u32 = 14;
        let mut values = Vec::new();
        let mut insts = Vec::new();
        for i in 0..n {
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("v{i}")),
            });
            insts.push(Inst {
                result: Some(ValueId(i)),
                kind: InstKind::Call(f0, Vec::new()),
                origin: None,
            });
        }
        // reduce: acc = v0; acc = acc + v_i for i in 1..n. Each v_i is
        // consumed late (slot n-1+i), so it stays live across the
        // intervening calls → call-crossing.
        let mut acc = Operand::Value(ValueId(0));
        for i in 1..n {
            let nv = ValueId(n + i - 1);
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("a{i}")),
            });
            insts.push(Inst {
                result: Some(nv),
                kind: InstKind::BinOp(BinOp::Add, acc, Operand::Value(ValueId(i))),
                origin: None,
            });
            acc = Operand::Value(nv);
        }
        let func = Function {
            name: "many_crossing".into(),
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

        let total_vals = n + (n - 1);
        let mut spill_offsets: Vec<u32> = Vec::new();
        for i in 0..total_vals {
            match alloc.of(ValueId(i)) {
                Reg::Gpr(_) => {}
                Reg::SpillGpr(off) => spill_offsets.push(off),
                other => panic!("v{i} got unexpected {other:?}"),
            }
        }
        spill_offsets.sort();
        spill_offsets.dedup();
        assert!(
            !spill_offsets.is_empty(),
            "callee-saved pool overflow must produce >=1 spill"
        );
        assert_eq!(alloc.total_spill_bytes, spill_offsets.len() as u32 * 8);
        assert!(
            alloc.used_callee_gpr_mask != 0,
            "call-crossing values must occupy callee-saved registers"
        );
    }

    /// FPR-pool exhaustion spills onto stack via SpillFpr. Build
    /// enough live f64 values to overflow the 13-slot FPR pool and
    /// verify SOME values land in SpillFpr slots + total_spill_bytes
    /// accounts for them.
    #[test]
    fn ls_spills_when_fpr_pool_exhausted() {
        // n f64 FAdd results all kept live to a final reduce chain.
        // The caller-saved (13) + callee-saved (8) FPR pools together
        // hold 21; n=24 simultaneously-live values overflow that,
        // forcing SpillFpr slots after both pools fill.
        let n: u32 = 24;
        let mut values = Vec::new();
        let mut insts = Vec::new();
        for i in 0..n {
            values.push(ValueInfo {
                ty: Type::F64,
                name: Some(format!("v{i}")),
            });
            insts.push(Inst {
                result: Some(ValueId(i)),
                kind: InstKind::BinOp(BinOp::FAdd, Operand::ConstF64(1.0), Operand::ConstF64(2.0)),
                origin: None,
            });
        }
        // Sum them all so they're live until the end.
        let mut acc = Operand::Value(ValueId(0));
        for i in 1..n {
            let nv = ValueId(n + i - 1);
            values.push(ValueInfo {
                ty: Type::F64,
                name: Some(format!("s{i}")),
            });
            insts.push(Inst {
                result: Some(nv),
                kind: InstKind::BinOp(BinOp::FAdd, acc, Operand::Value(ValueId(i))),
                origin: None,
            });
            acc = Operand::Value(nv);
        }
        let func = Function {
            name: "many_live_fp".into(),
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

        let total_vals = n + (n - 1);
        let mut spill_offsets: Vec<u32> = Vec::new();
        for i in 0..total_vals {
            match alloc.of(ValueId(i)) {
                Reg::Fpr(f) => assert!(
                    f == Fpr::V0
                        || aapcs64::FP_CALLER_SAVED_SCRATCH.contains(&f)
                        || aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f),
                    "v{i} got unexpected FPR {f:?}"
                ),
                Reg::SpillFpr(off) => spill_offsets.push(off),
                other => panic!("v{i} got unexpected {other:?}"),
            }
        }
        assert!(
            !spill_offsets.is_empty(),
            "caller+callee FPR pool overflow must spill"
        );
        spill_offsets.sort();
        spill_offsets.dedup();
        assert_eq!(
            alloc.total_spill_bytes,
            (spill_offsets.len() as u32) * 8,
            "spill_bytes accounts for every distinct SpillFpr slot"
        );
    }
}
