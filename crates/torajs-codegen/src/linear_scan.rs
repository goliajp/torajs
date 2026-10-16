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
//! ## Frame facts
//!
//! Alloca slot offsets, the `has_calls` flag and every call site's
//! slot index come from one walk (`linear_scan_calls::scan_frame`);
//! params take their AAPCS64 lanes (`X0..X7` and `V0..V7` counted
//! independently) from `linear_scan_lanes::param_lanes`.

use torajs_core::ssa::Function;
use torajs_core::ssa::block_freq::ENTRY_FREQ;

use crate::coalesce::build_copy_classes;
use crate::compile::InlineSites;
use crate::linear_scan_calls::{
    CallCosts, FrameScan, collect_call_saves, crosses_call, crosses_call_from_entry, scan_frame,
};
use crate::linear_scan_lanes::{param_entries, param_lanes, ret_lane_free};
use crate::linear_scan_sweep::Sweep;
use crate::live_ranges::LazyRanges;
use crate::liveness::compute_liveness;
use crate::reg::{Gpr, Reg, aapcs64};
use crate::regalloc::{Assignment, collect_ret_value_ids};
use crate::spill_weight::compute_spill_weights;

/// Linear-Scan register allocator with call-crossing awareness and
/// stack spill (see module docs).
pub fn allocate_linear_scan(func: &Function) -> Assignment {
    allocate_linear_scan_with(func, InlineSites::default())
}

/// [`allocate_linear_scan`] for a function whose intrinsic calls
/// `compile` opens inline: those sites call only on their slow arm.
pub fn allocate_linear_scan_with(func: &Function, inline: InlineSites) -> Assignment {
    allocate_linear_scan_reserving(func, inline, None)
}

/// [`allocate_linear_scan_with`] that hands `reserved` to no value
/// (the throw register of a function that reads it, compile/throw_reg.rs).
pub(crate) fn allocate_linear_scan_reserving(
    func: &Function,
    inline: InlineSites,
    reserved: Option<Gpr>,
) -> Assignment {
    let (mut intervals, blocks) = compute_liveness(func);
    let ret_vids = collect_ret_value_ids(func);

    let FrameScan {
        alloca_offsets,
        next_alloca_offset,
        has_calls,
        call_sites,
        call_rarity,
    } = scan_frame(func, inline);

    let (param_arg_reg, stack_params) = param_lanes(func);

    let classes = build_copy_classes(func, &intervals, &blocks, &ret_vids, &call_sites);
    let (mut weights, units) = compute_spill_weights(func);
    classes.fold_into(&mut intervals, &mut weights);
    let ranges = LazyRanges::new(func, &blocks, &intervals, &classes);
    let call_costs = CallCosts::new(&call_sites, &call_rarity, &units);
    let mut sweep = Sweep::new(next_alloca_offset, weights, call_costs, ranges, reserved);
    sweep.unspillable = crate::vec_homes::vector_values(func, &classes);
    sweep.log = crate::linear_scan_dump::wanted(func);

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
        let is_fp = ty.is_fpr_class();
        // Params def at function ENTRY — before inst slot 0 — so a
        // call at p == interval.start still clobbers their
        // caller-saved arg register. `crosses_call`'s `p == start`
        // exemption is only sound for the call's own result; for a
        // param it silently hands out a clobbered arg reg (post-
        // inline/slot-forward SSA exposed this: a param consumed
        // after a leading call read garbage from x1).
        let crossing = if param_arg_reg.contains_key(&vid) || stack_params.contains_key(&vid) {
            crosses_call_from_entry(interval, &call_sites)
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
            sweep.activate(vid, interval, dst);
            continue;
        }

        // Params first (they own a fixed AAPCS64 arg register on
        // entry, even when also the ret value — matching the prior
        // Pass-B-then-Pass-C ordering).
        if let Some(&arg_reg) = param_arg_reg.get(&vid) {
            if !crossing {
                // Stays in its arg register; a pool register must not
                // be handed out while the param is live.
                sweep.by_value.insert(vid, arg_reg);
                if sweep.claim(arg_reg) {
                    sweep.activate(vid, interval, arg_reg);
                }
                continue;
            }
            // Crosses a call — the caller-saved arg register would be
            // clobbered. Relocate to a callee-saved reg (or spill);
            // entry moves are derived from final `by_value` after the
            // sweep so a later `spill_at_active` eviction stays sound.
            let dst = sweep.alloc_crossing(vid, interval, is_fp);
            sweep.by_value.insert(vid, dst);
            sweep.activate(vid, interval, dst);
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
                let dst = if sweep.save_calls {
                    sweep.alloc_crossing_saved(vid, interval, is_fp)
                } else {
                    sweep.alloc_crossing(vid, interval, is_fp)
                };
                sweep.by_value.insert(vid, dst);
                sweep.activate(vid, interval, dst);
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

        sweep.alloc_value(vid, interval, is_fp, crossing, &call_sites);
    }

    sweep.rehome_saved();
    classes.fan_out(&mut sweep.by_value);
    if sweep.log {
        crate::linear_scan_dump::dump(func, &intervals, &sweep, &call_sites);
    }
    let (call_saves, vec_call_saves) = collect_call_saves(&mut sweep, &intervals, &call_sites);

    let total_spill_bytes = sweep.next_spill_offset - next_alloca_offset;
    let used_callee_gpr_mask = sweep.used_callee_gpr_mask;
    let used_callee_fpr_mask = sweep.used_callee_fpr_mask;

    let (param_entry_moves, stack_param_entry_loads) =
        param_entries(func, &sweep.by_value, &param_arg_reg, &stack_params);

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
    assignment.call_saves = call_saves;
    assignment.vec_call_saves = vec_call_saves;
    assignment.cold_blocks = cold_blocks(func, &units);
    crate::vec_homes::assert_vec_homes(func, &assignment);
    assignment
}

/// By block position, whether the block's estimated frequency (that of
/// its first slot in `units`) is at most a slow path's share of one
/// function entry.
fn cold_blocks(func: &Function, units: &[u64]) -> Vec<bool> {
    let slow = (ENTRY_FREQ as f64 * torajs_core::ssa::branch_prob::P_SLOW_PATH) as u64;
    let mut slot = 0usize;
    func.blocks
        .iter()
        .map(|b| {
            let cold = units.get(slot).is_some_and(|&f| f <= slow);
            slot += b.insts.len() + 1;
            cold
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::{Fpr, Gpr};
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, IPred, Inst, InstKind, Operand, Terminator, Type,
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
        for i in 0..26 {
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
        for i in 1..26 {
            let new_vid = ValueId(26 + i as u32 - 1);
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
        //   3. Spill and call-save slots together form a contiguous
        //      8-aligned cursor (no gaps, no overlap) starting at
        //      raw_alloca_bytes.
        //   4. total_spill_bytes == 8 × distinct slot count.
        let mut spill_offsets: Vec<u32> = Vec::new();
        for i in 0..51 {
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
        spill_offsets.extend(save_slots(&alloc));
        spill_offsets.sort();
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
            "spill_bytes accounts for every distinct slot"
        );
        assert_eq!(alloc.raw_alloca_bytes, 0);
    }

    /// Call-crossing values that overflow the 10-slot callee-saved
    /// pool take the 11 caller-saved registers (saved around calls),
    /// then spill via the "spill at active" path. We build 26 call
    /// results all kept live by a final reduce chain — every result
    /// crosses the later calls. Asserts the slot cursor is contiguous
    /// + accounted and that the callee-saved pool was exercised.
    #[test]
    fn ls_spills_self_when_new_arrival_has_furthest_end() {
        let f0 = FuncId(0);
        let n: u32 = 26;
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
        let slots = spill_offsets.len() + save_slots(&alloc).len();
        assert_eq!(alloc.total_spill_bytes, slots as u32 * 8);
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

    /// A parameter kept in its argument register holds it: with the
    /// pool spilling into X1..X8, no value live alongside the param may
    /// be handed its register.
    #[test]
    fn param_kept_in_arg_register_is_not_shared() {
        let n: u32 = 12;
        let (p0, p1) = (ValueId(0), ValueId(1));
        let mut insts: Vec<Inst> = (0..n)
            .map(|i| Inst {
                result: Some(ValueId(2 + i)),
                kind: InstKind::BinOp(BinOp::Add, Operand::Value(p0), Operand::ConstI64(i as i64)),
                origin: None,
            })
            .collect();
        let mut acc = ValueId(1);
        for i in 0..n {
            let next = ValueId(2 + n + i);
            insts.push(Inst {
                result: Some(next),
                kind: InstKind::BinOp(
                    BinOp::Add,
                    Operand::Value(acc),
                    Operand::Value(ValueId(2 + i)),
                ),
                origin: None,
            });
            acc = next;
        }
        let func = Function {
            name: "f".into(),
            params: vec![p0, p1],
            ret: Type::I64,
            values: (0..2 + 2 * n)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(Operand::Value(acc))),
            }],
            current_origin: None,
        };
        let a = allocate_linear_scan(&func);
        assert_eq!(a.of(p1), Reg::Gpr(Gpr::X1));
        for v in 2..2 + n {
            assert_ne!(a.of(ValueId(v)), Reg::Gpr(Gpr::X1), "v{v}");
        }
    }

    /// Distinct stack slots the call-site saves use.
    fn save_slots(alloc: &Assignment) -> Vec<u32> {
        let mut v: Vec<u32> = alloc
            .call_saves
            .values()
            .flatten()
            .map(|&(_, off)| off)
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Past the callee-saved pool, a call-crossing value whose calls
    /// are colder than its uses takes a caller-saved register and is
    /// saved around every later call it crosses — and only those.
    /// Thirteen call results at depth 0, each read in a loop body.
    #[test]
    fn crossing_values_past_the_callee_pool_are_saved_around_calls() {
        let f0 = FuncId(0);
        let n: u32 = 13;
        let mut values: Vec<ValueInfo> = Vec::new();
        let mut entry = Vec::new();
        for i in 0..n {
            values.push(ValueInfo {
                ty: Type::I64,
                name: None,
            });
            entry.push(Inst {
                result: Some(ValueId(i)),
                kind: InstKind::Call(f0, Vec::new()),
                origin: None,
            });
        }
        let mut body = Vec::new();
        let mut acc = Operand::Value(ValueId(0));
        for i in 1..n {
            let nv = ValueId(n + i - 1);
            values.push(ValueInfo {
                ty: Type::I64,
                name: None,
            });
            body.push(Inst {
                result: Some(nv),
                kind: InstKind::BinOp(BinOp::Add, acc, Operand::Value(ValueId(i))),
                origin: None,
            });
            acc = Operand::Value(nv);
        }
        let cond = ValueId(2 * n - 1);
        values.push(ValueInfo {
            ty: Type::Bool,
            name: None,
        });
        body.push(Inst {
            result: Some(cond),
            kind: InstKind::ICmp(IPred::Slt, acc, Operand::ConstI64(0)),
            origin: None,
        });
        let func = Function {
            name: "saved".into(),
            params: Vec::new(),
            ret: Type::I64,
            values,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: entry,
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: body,
                    term: Terminator::CondBr {
                        cond: Operand::Value(cond),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(2),
                    insts: Vec::new(),
                    term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
                },
            ],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        let mut saved_regs = 0;
        for i in 0..n {
            let reg = alloc.of(ValueId(i));
            let Reg::Gpr(g) = reg else { continue };
            if !aapcs64::CALLER_SAVED_SCRATCH.contains(&g) {
                continue;
            }
            saved_regs += 1;
            // v_i is defined by the call at slot i and crosses the calls
            // at slots i+1 .. n-1
            for p in 0..n {
                let saved = alloc
                    .call_saves
                    .get(&p)
                    .is_some_and(|v| v.iter().any(|&(r, _)| r == reg));
                assert_eq!(saved, p > i, "v{i} in {g:?} at call {p}");
            }
        }
        assert!(saved_regs > 0, "some value must overflow the callee pool");
        assert_eq!(alloc.used_callee_gpr_mask.count_ones(), 10);
    }

    /// A value live into a block whose first instruction is a call is
    /// live across that call even though its interval starts there:
    /// layout bb0 → bb1 (call; → bb4) → bb3 (def v1; → bb1) → bb4
    /// (use v1). v1's uses all follow its def, so only the live-in
    /// start says it is live at bb1's call.
    #[test]
    fn value_live_into_a_block_that_opens_with_a_call_crosses_it() {
        let (p0, v1, v2) = (ValueId(0), ValueId(1), ValueId(2));
        let func = Function {
            name: "f".into(),
            params: vec![p0],
            ret: Type::I64,
            values: (0..3)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![],
                    term: Terminator::Br(BlockId(3)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: None,
                        kind: InstKind::Call(FuncId(0), Vec::new()),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(4)),
                },
                Block {
                    id: BlockId(3),
                    insts: vec![Inst {
                        result: Some(v1),
                        kind: InstKind::BinOp(BinOp::Add, Operand::Value(p0), Operand::ConstI64(1)),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(4),
                    insts: vec![Inst {
                        result: Some(v2),
                        kind: InstKind::BinOp(BinOp::Add, Operand::Value(v1), Operand::ConstI64(1)),
                        origin: None,
                    }],
                    term: Terminator::Ret(Some(Operand::Value(v2))),
                },
            ],
            current_origin: None,
        };
        let a = allocate_linear_scan(&func);
        match a.of(v1) {
            Reg::Gpr(g) => assert!(aapcs64::CALLEE_SAVED_SCRATCH.contains(&g), "{g:?}"),
            other => panic!("{other:?}"),
        }
    }
}
