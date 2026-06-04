//! Linear Scan register allocation (Poletto & Sarkar 1999) — phase
//! 2 + 3 of the LS pipeline, without spill.
//!
//! Phase 2 = active-set sweep:
//!
//!   1. Sort all interval starts ascending.
//!   2. Walk in that order; before each new interval,
//!      *expire* any active interval whose `end < current.start`
//!      (its register returns to the free pool).
//!   3. Allocate the new interval a register from the appropriate
//!      class's free pool. If empty, panic for now — stack spill
//!      lands in sub-step 3.
//!
//! Phase 3 = stack spill — **NOT YET IMPLEMENTED**. When a pool is
//! exhausted this allocator currently panics. The next sub-step
//! introduces `Reg::Stack(byte_offset)` and rewires `emit_*` to
//! `LDR scratch, [SP, #off]` on operand fetch and `STR scratch,
//! [SP, #off]` on def write-back. After that, no SSA shape causes
//! `allocate_linear_scan` to panic.
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
//! the same answer (both consume from `X12, X13, ...` in inst-def
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
                // Keeps register pressure tight (the same X12 is
                // recycled across non-overlapping intervals rather
                // than spreading writes across X12..X22).
                match active[i].2 {
                    Reg::Gpr(g) => free_gpr.push_front(g),
                    Reg::Fpr(f) => free_fpr.push_front(f),
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
                None => panic!(
                    "LS-2: FPR pool exhausted at ValueId({vid}) — stack spill lands in S5 LS sub-step 3"
                ),
            }
        } else {
            match free_gpr.pop_front() {
                Some(g) => Reg::Gpr(g),
                None => panic!(
                    "LS-2: GPR pool exhausted at ValueId({vid}) — stack spill lands in S5 LS sub-step 3"
                ),
            }
        };

        by_value.insert(vid, reg);
        // Only scratch-pool-backed values join the active set; the
        // ret lane lives outside the pool and would mislead expiry.
        if !is_ret {
            active.push((vid, interval, reg));
        }
    }

    Assignment::from_parts(by_value, alloca_offsets, next_alloca_offset, has_calls)
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
    /// three the same scratch (X12, first in CALLER_SAVED_SCRATCH).
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
        // All three should land in X12 (first scratch) because each
        // expires before the next claim.
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X12));
        assert_eq!(alloc.of(v1), Reg::Gpr(Gpr::X12));
        assert_eq!(alloc.of(v2), Reg::Gpr(Gpr::X12));
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
    /// LS should give v0=X12, v1=X13 (both alive at slot 2 when v2
    /// is defined), v2=X0 (ret lane).
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
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X12));
        assert_eq!(alloc.of(v1), Reg::Gpr(Gpr::X13));
        assert_eq!(alloc.of(v2), Reg::Gpr(Gpr::X0));
    }

    /// FPR + GPR separate pools — fp value gets V16 (first FP
    /// scratch), int value gets X12 (first GPR scratch).
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
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X12));
        assert_eq!(alloc.of(v1), Reg::Fpr(Fpr::V16));
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

    /// LS must panic with a clear "spill not implemented" message
    /// when the GPR pool is exhausted (12 simultaneously-live int
    /// values — one more than the 11-slot CALLER_SAVED_SCRATCH pool).
    /// Sub-step 3 will replace this panic with stack spill.
    #[test]
    #[should_panic(expected = "GPR pool exhausted")]
    fn ls_panics_when_gpr_pool_exhausted() {
        // Build 12 non-ret call results all kept alive simultaneously
        // by using each one in a final binop chain. The 12th claim
        // exceeds CALLER_SAVED_SCRATCH.len() == 11.
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
        // Final inst reads every v_i so all 12 stay alive
        // simultaneously up to the use site. The "ret" inst itself
        // gets X0 (ret lane), but its operands' intervals all
        // extend to that point, so 12 scratch slots are needed for
        // the chain — and only 11 are available.
        //
        // We use a chained BinOp pattern: ret = ((((v0+v1)+v2)+...)+v11).
        // That adds 11 more SSA defs, but each is consumed by the
        // next BinOp so they don't all overlap. The 12 original
        // v_i's are what actually crowd the pool.
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
        let _ = allocate_linear_scan(&func);
    }
}
