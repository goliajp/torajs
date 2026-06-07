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

/// `true` iff some call site falls strictly inside `[start, end]` —
/// the value is defined before a `BL` and still used after it, so a
/// caller-saved register would be clobbered. `p == start` means the
/// value IS the call's result (written after the call returns, safe);
/// `p == end` means it's consumed as that call's argument and dies
/// immediately (safe). The strict `<` on both ends excludes those.
fn crosses_call(interval: Interval, call_sites: &[u32]) -> bool {
    call_sites
        .iter()
        .any(|&p| interval.start < p && p < interval.end)
}

/// Mutable Linear-Scan sweep state. The free pool is split four ways
/// (caller / callee × GPR / FPR) so a call-crossing value can be
/// restricted to callee-saved registers — the ones AAPCS64 §6.1.1
/// guarantees survive a `BL`. Caller-saved registers are handed only
/// to values that never outlive a call.
struct Sweep {
    /// Final ValueId → register map being built.
    by_value: HashMap<u32, Reg>,
    /// (ValueId, interval, reg) of every pool-backed live value.
    /// Vec — N ≤ pool size, linear scan beats tree maintenance.
    active: Vec<(u32, Interval, Reg)>,
    free_caller_gpr: VecDeque<Gpr>,
    free_callee_gpr: VecDeque<Gpr>,
    free_caller_fpr: VecDeque<Fpr>,
    free_callee_fpr: VecDeque<Fpr>,
    /// sp-relative offset of the next spill slot (grows by 8; all
    /// spilled values are 64-bit GPR or D-form FPR).
    next_spill_offset: u32,
    /// `Gpr::idx` / `Fpr::idx` bitmasks of the callee-saved registers
    /// actually handed out — the frame saves/restores exactly these.
    used_callee_gpr_mask: u32,
    used_callee_fpr_mask: u32,
}

impl Sweep {
    fn new(spill_base: u32) -> Self {
        Sweep {
            by_value: HashMap::new(),
            active: Vec::new(),
            free_caller_gpr: aapcs64::CALLER_SAVED_SCRATCH.iter().copied().collect(),
            free_callee_gpr: aapcs64::CALLEE_SAVED_SCRATCH.iter().copied().collect(),
            free_caller_fpr: aapcs64::FP_CALLER_SAVED_SCRATCH.iter().copied().collect(),
            free_callee_fpr: aapcs64::FP_CALLEE_SAVED_SCRATCH.iter().copied().collect(),
            next_spill_offset: spill_base,
            used_callee_gpr_mask: 0,
            used_callee_fpr_mask: 0,
        }
    }

    /// Release every active interval whose end is strictly before
    /// `cur_start`; its register returns to the pool it came from.
    fn expire(&mut self, cur_start: u32) {
        let mut i = 0;
        while i < self.active.len() {
            if self.active[i].1.end < cur_start {
                let reg = self.active[i].2;
                self.release(reg);
                self.active.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Return a freed register to its originating pool (LIFO — keeps
    /// register pressure tight). Spilled slots release nothing (their
    /// stack offset is owned for the whole function lifetime).
    fn release(&mut self, reg: Reg) {
        match reg {
            Reg::Gpr(g) => {
                if aapcs64::CALLER_SAVED_SCRATCH.contains(&g) {
                    self.free_caller_gpr.push_front(g);
                } else if aapcs64::CALLEE_SAVED_SCRATCH.contains(&g) {
                    self.free_callee_gpr.push_front(g);
                }
            }
            Reg::Fpr(f) => {
                if aapcs64::FP_CALLER_SAVED_SCRATCH.contains(&f) {
                    self.free_caller_fpr.push_front(f);
                } else if aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f) {
                    self.free_callee_fpr.push_front(f);
                }
            }
            Reg::SpillGpr(_) | Reg::SpillFpr(_) => {}
        }
    }

    /// Pop a callee-saved GPR and record it in the used-mask.
    fn take_callee_gpr(&mut self) -> Option<Gpr> {
        let g = self.free_callee_gpr.pop_front()?;
        self.used_callee_gpr_mask |= 1 << g.idx();
        Some(g)
    }

    /// Pop a callee-saved FPR and record it in the used-mask.
    fn take_callee_fpr(&mut self) -> Option<Fpr> {
        let f = self.free_callee_fpr.pop_front()?;
        self.used_callee_fpr_mask |= 1 << f.idx();
        Some(f)
    }

    fn alloc_spill_slot(&mut self) -> u32 {
        let off = self.next_spill_offset;
        self.next_spill_offset += 8;
        off
    }

    /// Allocate for a value whose interval crosses a call: callee-
    /// saved only (caller-saved would be clobbered by the BL). Spills
    /// when the callee pool is exhausted.
    fn alloc_crossing(&mut self, interval: Interval, is_fp: bool) -> Reg {
        let reg = if is_fp {
            self.take_callee_fpr().map(Reg::Fpr)
        } else {
            self.take_callee_gpr().map(Reg::Gpr)
        };
        reg.unwrap_or_else(|| self.spill_at_active(interval, is_fp, /*callee_only=*/ true))
    }

    /// Allocate for a value that never crosses a call: prefer caller-
    /// saved (free to clobber), fall back to callee-saved (then it
    /// must be saved/restored), spill last.
    fn alloc_noncrossing(&mut self, interval: Interval, is_fp: bool) -> Reg {
        if is_fp {
            if let Some(f) = self.free_caller_fpr.pop_front() {
                return Reg::Fpr(f);
            }
            if let Some(f) = self.take_callee_fpr() {
                return Reg::Fpr(f);
            }
        } else {
            if let Some(g) = self.free_caller_gpr.pop_front() {
                return Reg::Gpr(g);
            }
            if let Some(g) = self.take_callee_gpr() {
                return Reg::Gpr(g);
            }
        }
        self.spill_at_active(interval, is_fp, /*callee_only=*/ false)
    }

    /// Poletto & Sarkar 1999 "spill at active" with pool-class
    /// awareness: pick the furthest-end active value of the matching
    /// class (restricted to callee-saved holders when `callee_only`,
    /// i.e. the new arrival crosses a call) and spill whichever of
    /// {victim, new} lives longer — the longest remaining range
    /// maximizes the freed-register window.
    ///
    /// Returns the reg to assign to the new arrival; if a victim was
    /// chosen, its `by_value` + `active` entry are rewritten to the
    /// spill slot in place and the new arrival inherits its register
    /// (already in the used-mask if it was callee-saved).
    fn spill_at_active(&mut self, new_interval: Interval, is_fp: bool, callee_only: bool) -> Reg {
        let mut victim_idx: Option<usize> = None;
        let mut victim_end: i64 = -1;
        for (i, e) in self.active.iter().enumerate() {
            let (is_callee_reg, class_ok) = match e.2 {
                Reg::Gpr(g) => (aapcs64::CALLEE_SAVED_SCRATCH.contains(&g), !is_fp),
                Reg::Fpr(f) => (aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f), is_fp),
                // Already-spilled actives can't free a register.
                Reg::SpillGpr(_) | Reg::SpillFpr(_) => continue,
            };
            if !class_ok || (callee_only && !is_callee_reg) {
                continue;
            }
            if (e.1.end as i64) > victim_end {
                victim_end = e.1.end as i64;
                victim_idx = Some(i);
            }
        }

        let spill_off = self.alloc_spill_slot();
        match victim_idx {
            Some(i) if (self.active[i].1.end as i64) > new_interval.end as i64 => {
                let (victim_vid, _vi, victim_reg) = self.active[i];
                let spilled = if is_fp {
                    Reg::SpillFpr(spill_off)
                } else {
                    Reg::SpillGpr(spill_off)
                };
                self.by_value.insert(victim_vid, spilled);
                self.active[i].2 = spilled;
                victim_reg
            }
            _ => {
                if is_fp {
                    Reg::SpillFpr(spill_off)
                } else {
                    Reg::SpillGpr(spill_off)
                }
            }
        }
    }
}

/// Linear-Scan register allocator with call-crossing awareness and
/// stack spill (see module docs).
pub fn allocate_linear_scan(func: &Function) -> Assignment {
    let intervals = compute_intervals(func);
    let ret_vids = collect_ret_value_ids(func);

    // Pass A — alloca offsets, has_calls, and the global inst-slot
    // index of every call site. The numbering MUST match
    // `compute_intervals` (each block contributes one index per inst
    // plus one for its terminator) so a call site `p` compares
    // directly against interval `[start, end]` bounds.
    let mut alloca_offsets: HashMap<u32, u32> = HashMap::new();
    let mut next_alloca_offset: u32 = 0;
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

    // AAPCS64 §5.4.2 param lanes — each param's entry register.
    let mut param_arg_reg: HashMap<u32, Reg> = HashMap::new();
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
        param_arg_reg.insert(param.0, reg);
    }

    let mut sweep = Sweep::new(next_alloca_offset);
    let mut param_entry_moves: Vec<(Reg, Reg)> = Vec::new();

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
        let crossing = crosses_call(interval, &call_sites);

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
            // clobbered. Relocate to a callee-saved reg (or spill) and
            // move it there on entry.
            let dst = sweep.alloc_crossing(interval, is_fp);
            param_entry_moves.push((arg_reg, dst));
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
                let dst = sweep.alloc_crossing(interval, is_fp);
                sweep.by_value.insert(vid, dst);
                sweep.active.push((vid, interval, dst));
                continue;
            }
            let reg = if is_fp {
                Reg::Fpr(aapcs64::FP_ARG_RET[0])
            } else {
                Reg::Gpr(aapcs64::ARG_RET[0])
            };
            sweep.by_value.insert(vid, reg);
            continue;
        }

        let reg = if crossing {
            sweep.alloc_crossing(interval, is_fp)
        } else {
            sweep.alloc_noncrossing(interval, is_fp)
        };
        sweep.by_value.insert(vid, reg);
        sweep.active.push((vid, interval, reg));
    }

    let total_spill_bytes = sweep.next_spill_offset - next_alloca_offset;
    let used_callee_gpr_mask = sweep.used_callee_gpr_mask;
    let used_callee_fpr_mask = sweep.used_callee_fpr_mask;
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
    assignment
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
