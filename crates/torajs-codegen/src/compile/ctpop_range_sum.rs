//! `CtpopRangeSum` canned emit — the 8-wide SIMD popcount-reduction
//! loop (RFC 20260719-ctpop-range-sum blade 1b).
//!
//! Mirrors the shape LLVM emits for the rust popcount comparator
//! (decomp doc §3): per iteration 4 groups of
//! `cnt.16b → udot.4s(weight 0x01…01) → uadalp.2d` into 4 independent
//! 2×u64 accumulators, IV stepped in the vector domain, `subs + b.ne`
//! loop control. Two deliberate deviations for register economy (the
//! canned sequence must fit the non-allocatable FPR set):
//!
//!   - the 3 derived IV vectors (`{i+2,i+3}` / `{i+4,i+5}` /
//!     `{i+6,i+7}`) roll through ONE register (V17) via serial
//!     `add.2d` instead of rust's 3 pre-splat step vectors — same
//!     instruction count, per-iteration side chain, OoO covers the
//!     added latency;
//!   - a scalar IV cursor (X9) advances alongside the vector IV so
//!     the tail loop and its bound check need no extra bookkeeping
//!     register.
//!
//! Register contract (all outside the allocator's pools — see
//! `reg::aapcs64`): GPR scratches X9-X12; FPR V0-V3 (accumulators),
//! V4 (IV lane pair), V5 (weight), V6 (step2), V7 (step8), V16-V18
//! (cnt / rolling IV / udot scratches). V0-V7 may home NON-crossing
//! params, so `regalloc::inst_emits_bl` marks this inst a clobber
//! site — the allocator relocates every live-across value to a
//! callee-saved home exactly as for a Call.
//!
//! Semantics: `acc_init + Σ ctpop(i as u64) for i in [start, bound)`
//! (signed i64 range; empty when start ≥ bound). Formation
//! precondition: `bound - start` must not overflow i64 — guaranteed
//! for loops that actually terminate in bounded time.

use torajs_core::ssa::{Inst, Operand};

use super::operand::materialize_operand_gpr;
use super::{
    OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP, write_def_spill_gpr,
    write_u32,
};
use crate::enc::{
    add_imm, add_reg, add_v2d, addp_d_v2d, addv_b_v8b, b_cond_imm19, cbz_x, cmp_imm, cmp_reg,
    cnt_v8b, cnt_v16b, cond, dup_2d_x, fmov_d_from_x, fmov_x_from_d, ins_d1_x, lsrv_reg, mov_x_reg,
    movi_2d_zero, movz_imm, sub_reg, subs_imm, uadalp_2d, udot_4s,
};
use crate::reg::{Fpr, Gpr};
use crate::regalloc::Assignment;

/// All-ones byte weight for `udot` — dot against it horizontally
/// sums each 4-byte group of the cnt output into a u32 lane.
const WEIGHT_ONES: i64 = 0x0101_0101_0101_0101;

/// Overwrite the placeholder word at `site` with `word`.
fn patch_word(bytes: &mut [u8], site: usize, word: u32) {
    bytes[site..site + 4].copy_from_slice(&word.to_le_bytes());
}

/// Emit one 4-op reduction group: `cnt.16b` the lane pair in `iv`,
/// zero the udot scratch, dot against the weight vector, widen into
/// accumulator `acc`.
fn emit_group(bytes: &mut Vec<u8>, iv: Fpr, acc: Fpr) {
    write_u32(bytes, cnt_v16b(Fpr::V16, iv));
    write_u32(bytes, movi_2d_zero(Fpr::V18));
    write_u32(bytes, udot_4s(Fpr::V18, Fpr::V16, Fpr::V5));
    write_u32(bytes, uadalp_2d(acc, Fpr::V18));
}

pub fn emit_ctpop_range_sum(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    start: &Operand,
    bound: &Operand,
    acc_init: &Operand,
    alloc: &Assignment,
) {
    // Working registers. X10 doubles as bound capture → group
    // down-counter → re-materialized bound for the tail; X12 as trip
    // → preamble temp → tail ctpop temp → result scratch.
    const IV: Gpr = OP_SCRATCH_LHS; // X9
    const CNT: Gpr = OP_SCRATCH_RHS; // X10
    const ACC: Gpr = OP_SCRATCH_TMP; // X11
    const TMP: Gpr = OP_SCRATCH_RESULT_GPR; // X12

    // -- capture: own the IV cursor and the running accumulator.
    // Operand homes are never X9-X12 (reserved out of the pools), so
    // the copies can't clobber each other's sources.
    let rs = materialize_operand_gpr(bytes, start, IV, alloc);
    if rs != IV {
        write_u32(bytes, mov_x_reg(IV, rs));
    }
    let ra = materialize_operand_gpr(bytes, acc_init, ACC, alloc);
    if ra != ACC {
        write_u32(bytes, mov_x_reg(ACC, ra));
    }
    let rb = materialize_operand_gpr(bytes, bound, CNT, alloc);

    // trip = bound - start; nothing to do when ≤ 0.
    write_u32(bytes, sub_reg(TMP, rb, IV));
    write_u32(bytes, cmp_imm(TMP, 0));
    let fix_done_a = bytes.len();
    write_u32(bytes, b_cond_imm19(cond::LE, 0)); // → DONE

    // ngroups = trip >> 3; skip the vector body when < 8 elements.
    write_u32(bytes, movz_imm(CNT, 3, 0));
    write_u32(bytes, lsrv_reg(CNT, TMP, CNT));
    let fix_tail = bytes.len();
    write_u32(bytes, cbz_x(CNT, 0)); // → TAIL

    // -- vector preamble: IV lane pair {i, i+1}, weight, steps, and
    // zeroed accumulators.
    write_u32(bytes, dup_2d_x(Fpr::V4, IV));
    write_u32(bytes, add_imm(TMP, IV, 1));
    write_u32(bytes, ins_d1_x(Fpr::V4, TMP));
    write_u32(bytes, movz_imm(TMP, 2, 0));
    write_u32(bytes, dup_2d_x(Fpr::V6, TMP));
    write_u32(bytes, movz_imm(TMP, 8, 0));
    write_u32(bytes, dup_2d_x(Fpr::V7, TMP));
    super::materialize_const_i64(bytes, TMP, WEIGHT_ONES);
    write_u32(bytes, dup_2d_x(Fpr::V5, TMP));
    for acc in [Fpr::V0, Fpr::V1, Fpr::V2, Fpr::V3] {
        write_u32(bytes, movi_2d_zero(acc));
    }

    // -- vector loop: 8 elements / iteration.
    let vloop_head = bytes.len();
    emit_group(bytes, Fpr::V4, Fpr::V0); // {i, i+1}
    write_u32(bytes, add_v2d(Fpr::V17, Fpr::V4, Fpr::V6));
    emit_group(bytes, Fpr::V17, Fpr::V1); // {i+2, i+3}
    write_u32(bytes, add_v2d(Fpr::V17, Fpr::V17, Fpr::V6));
    emit_group(bytes, Fpr::V17, Fpr::V2); // {i+4, i+5}
    write_u32(bytes, add_v2d(Fpr::V17, Fpr::V17, Fpr::V6));
    emit_group(bytes, Fpr::V17, Fpr::V3); // {i+6, i+7}
    write_u32(bytes, add_v2d(Fpr::V4, Fpr::V4, Fpr::V7)); // iv += 8
    write_u32(bytes, add_imm(IV, IV, 8)); // scalar cursor mirrors
    write_u32(bytes, subs_imm(CNT, CNT, 1));
    let back = vloop_head as i32 - bytes.len() as i32;
    write_u32(bytes, b_cond_imm19(cond::NE, back));

    // -- merge: pairwise-fold the 4 accumulators, fold the lane pair,
    // add into the running scalar sum.
    write_u32(bytes, add_v2d(Fpr::V0, Fpr::V0, Fpr::V1));
    write_u32(bytes, add_v2d(Fpr::V2, Fpr::V2, Fpr::V3));
    write_u32(bytes, add_v2d(Fpr::V0, Fpr::V0, Fpr::V2));
    write_u32(bytes, addp_d_v2d(Fpr::V0, Fpr::V0));
    write_u32(bytes, fmov_x_from_d(TMP, Fpr::V0));
    write_u32(bytes, add_reg(ACC, ACC, TMP));

    // -- TAIL: remaining trip % 8 elements via the scalar ctpop
    // sequence. The bound re-materializes into X10 (free since the
    // down-counter hit zero; a Value operand's home reg is untouched
    // by the canned body, so this is a no-op lookup for it).
    let tail_pos = bytes.len();
    patch_word(
        bytes,
        fix_tail,
        cbz_x(CNT, tail_pos as i32 - fix_tail as i32),
    );
    let rb2 = materialize_operand_gpr(bytes, bound, CNT, alloc);
    write_u32(bytes, cmp_reg(IV, rb2));
    let fix_done_c = bytes.len();
    write_u32(bytes, b_cond_imm19(cond::GE, 0)); // → DONE
    let tloop_head = bytes.len();
    write_u32(bytes, fmov_d_from_x(Fpr::V16, IV));
    write_u32(bytes, cnt_v8b(Fpr::V16, Fpr::V16));
    write_u32(bytes, addv_b_v8b(Fpr::V16, Fpr::V16));
    write_u32(bytes, fmov_x_from_d(TMP, Fpr::V16));
    write_u32(bytes, add_reg(ACC, ACC, TMP));
    write_u32(bytes, add_imm(IV, IV, 1));
    write_u32(bytes, cmp_reg(IV, rb2));
    let back = tloop_head as i32 - bytes.len() as i32;
    write_u32(bytes, b_cond_imm19(cond::LT, back));

    // -- DONE: the running sum in X11 is the result.
    let done_pos = bytes.len();
    patch_word(
        bytes,
        fix_done_a,
        b_cond_imm19(cond::LE, done_pos as i32 - fix_done_a as i32),
    );
    patch_word(
        bytes,
        fix_done_c,
        b_cond_imm19(cond::GE, done_pos as i32 - fix_done_c as i32),
    );
    let result_vid = inst.result.expect("CtpopRangeSum must have a result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, TMP);
    write_u32(bytes, mov_x_reg(dst, ACC));
    write_def_spill_gpr(bytes, spill_off, dst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_function;
    use crate::enc;
    use crate::linear_scan::allocate_linear_scan;
    use crate::reg::{Reg, aapcs64};
    use torajs_core::ssa::{
        Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    };

    fn range_sum_func(start: i64, bound: i64, acc: i64) -> Function {
        let v0 = ValueId(0);
        Function {
            name: "ctpop_range_sum_smoke".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("sum".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::CtpopRangeSum(
                        Operand::ConstI64(start),
                        Operand::ConstI64(bound),
                        Operand::ConstI64(acc),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// The fixed-register vector loop body (head through b.ne) must
    /// appear verbatim in the compiled stream — 23 words, backward
    /// displacement -88.
    #[test]
    fn canned_vector_loop_body_appears_verbatim() {
        let compiled = compile_function(&range_sum_func(0, 100, 0));

        let mut body: Vec<u8> = Vec::new();
        emit_group(&mut body, Fpr::V4, Fpr::V0);
        crate::compile::write_u32(&mut body, enc::add_v2d(Fpr::V17, Fpr::V4, Fpr::V6));
        emit_group(&mut body, Fpr::V17, Fpr::V1);
        crate::compile::write_u32(&mut body, enc::add_v2d(Fpr::V17, Fpr::V17, Fpr::V6));
        emit_group(&mut body, Fpr::V17, Fpr::V2);
        crate::compile::write_u32(&mut body, enc::add_v2d(Fpr::V17, Fpr::V17, Fpr::V6));
        emit_group(&mut body, Fpr::V17, Fpr::V3);
        crate::compile::write_u32(&mut body, enc::add_v2d(Fpr::V4, Fpr::V4, Fpr::V7));
        crate::compile::write_u32(&mut body, enc::add_imm(Gpr::X9, Gpr::X9, 8));
        crate::compile::write_u32(&mut body, enc::subs_imm(Gpr::X10, Gpr::X10, 1));
        crate::compile::write_u32(&mut body, enc::b_cond_imm19(cond::NE, -(22 * 4)));
        assert_eq!(body.len(), 23 * 4);

        let hits = compiled
            .bytes
            .windows(body.len())
            .filter(|w| *w == body.as_slice())
            .count();
        assert_eq!(hits, 1, "canned vector loop body must appear exactly once");
        assert!(compiled.relocs.is_empty(), "canned loop needs no relocs");
        let last = u32::from_le_bytes(
            compiled.bytes[compiled.bytes.len() - 4..]
                .try_into()
                .unwrap(),
        );
        assert_eq!(last, enc::ret(Gpr::X30));
    }

    /// The scalar tail loop (fmov/cnt.8b/addv/fmov + accumulate +
    /// step + bound check) must also be present for the trip%8 rest.
    #[test]
    fn canned_tail_loop_present() {
        let compiled = compile_function(&range_sum_func(0, 100, 0));
        let mut tail: Vec<u8> = Vec::new();
        for w in [
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            enc::cnt_v8b(Fpr::V16, Fpr::V16),
            enc::addv_b_v8b(Fpr::V16, Fpr::V16),
            enc::fmov_x_from_d(Gpr::X12, Fpr::V16),
            enc::add_reg(Gpr::X11, Gpr::X11, Gpr::X12),
            enc::add_imm(Gpr::X9, Gpr::X9, 1),
        ] {
            crate::compile::write_u32(&mut tail, w);
        }
        let hits = compiled
            .bytes
            .windows(tail.len())
            .filter(|w| *w == tail.as_slice())
            .count();
        assert_eq!(hits, 1, "scalar tail body must appear exactly once");
    }

    /// Allocator contract: the inst is an `inst_emits_bl` clobber
    /// site, so an f64 param live across it must NOT stay homed in
    /// its V0-V7 arg register (the canned loop clobbers those) —
    /// it relocates to a callee-saved FPR or a spill slot.
    #[test]
    fn f64_param_live_across_relocates_out_of_arg_reg() {
        let param = ValueId(0);
        let sum = ValueId(1);
        let func = Function {
            name: "f64_param_crossing".into(),
            params: vec![param],
            ret: Type::F64,
            values: vec![
                ValueInfo {
                    ty: Type::F64,
                    name: Some("p".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("sum".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(sum),
                    kind: InstKind::CtpopRangeSum(
                        Operand::ConstI64(0),
                        Operand::ConstI64(64),
                        Operand::ConstI64(0),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(param))),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        match alloc.of(param) {
            Reg::Fpr(f) => assert!(
                aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f),
                "param must relocate to a callee-saved FPR, got {f:?}"
            ),
            Reg::SpillFpr(_) => {}
            other => panic!("f64 param in unexpected home {other:?}"),
        }
    }
}
