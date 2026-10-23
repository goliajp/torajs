//! AArch64 instruction encoders.
//!
//! Each emitter returns a `u32` instruction word in *host* byte
//! order; the function-level driver in `compile/` writes little-
//! endian bytes (aarch64 is little-endian) to the output stream.
//!
//! Every encoder has an inline unit test asserting the emitted word
//! bit-pattern matches the ARM ARM (DDI 0487) spec; this is the
//! ground-truth source for #6 — if every encoder is spec-conformant,
//! the only remaining failure mode upstream is wrong instruction
//! selection, which `compile/` is responsible for and catches via
//! its own end-to-end byte-equal tests.
//!
//! Split into sub-modules by ISA section to keep each prod file
//! under the 500-LOC hard limit:
//!
//!   - [`int`]: data-processing — MOVZ/MOVK, ADD/SUB (imm + reg),
//!     MUL/SDIV/UDIV/MSUB, AND/ORR/EOR (logical reg), LSLV/ASRV/
//!     LSRV (variable shift), CMP, AND-imm-#1, MOV reg-reg (W + X).
//!   - [`fp`]: floating-point — FADD/FSUB/FMUL/FDIV (D-form),
//!     FMOV both directions (general ↔ FP), FCMP, SCVTF + FCVTZS.
//!   - [`mem`]: loads + stores — LDR/STR imm12, LDR/STR register-
//!     index, STP/LDP pre/post-index for prologue/epilogue.
//!   - [`ctrl`]: control flow + immediate — RET, B/BL/BLR, B.cond,
//!     CBZ/CBNZ, BRK, ADRP, CSET, the 4-bit `cond::*` table.
//!   - [`simd`]: 128-bit Q-form loads / stores, ORR.16B, UMOV.
//!   - [`sys`]: system registers — the FPCR MRS/MSR pair the program
//!     entry uses to select default-NaN mode, and the single-bit
//!     logical immediate that sets its bit.

mod ctrl;
mod fp;
mod int;
mod mem;
mod simd;
mod sys;

pub use ctrl::{
    adrp, b_cond_imm19, b_imm26, bl_imm26, blr_reg, br_reg, brk_imm16, cb_w, cbnz_x, cbz_x, cond,
    csel_cond, cset_cond, csinc_cond, nop, ret, tbnz, tbz,
};
pub use fp::{
    add_v2d, addp_d_v2d, addv_b_v8b, cnt_v8b, cnt_v16b, dup_2d_x, fadd_d, fcmp_d, fcmp_d_zero,
    fcsel_d, fcvtzs_x_d, fdiv_d, fjcvtzs_w_d, fmov_d_from_x, fmov_x_from_d, fmul_d, fsub_d,
    ins_d1_x, ldr_d_imm12, ldr_d_reg, ldr_d_reg_lsl3, mov_d_reg, movi_2d_zero, scvtf_d_w,
    scvtf_d_x, str_d_imm12, str_d_reg, str_d_reg_lsl3, uadalp_2d, udot_4s,
};
pub use int::{
    add_imm, add_imm_lsl12, add_reg, add_reg_lsl, add_w_imm, and_imm, and_imm_one, and_reg,
    asr_imm, asrv_reg, clz_reg, cmp_imm, cmp_reg, cmp_w_imm, cmp_w_reg, cmp_x_w_sxtw, eor_imm,
    eor_reg, logical_imm, lsl_imm, lslv_reg, lsr_imm, lsrv_reg, mov_w_reg, mov_x_reg, movk_imm,
    movn_imm, movz_imm, msub_reg, mul_reg, orr_imm, orr_reg, rbit_reg, sdiv_reg, sub_imm,
    sub_imm_lsl12, sub_reg, sub_w_imm, subs_imm, sxtw_x_w, tst_imm, udiv_reg,
};
pub use mem::{
    ldp_post_index, ldp_x_off, ldr_w_imm12, ldr_w_reg, ldr_x_imm12, ldr_x_reg, ldr_x_reg_lsl3,
    ldrb_w_imm12, ldrb_w_reg, ldrh_w_imm12, ldrh_w_reg_lsl1, ldur_w_imm9, ldur_x_imm9,
    stp_pre_index, stp_w_off, stp_x_off, str_w_imm12, str_w_reg, str_x_imm12, str_x_reg,
    str_x_reg_lsl3, strb_w_imm12, stur_w_imm9, stur_x_imm9, sturh_w_imm9,
};
pub use simd::{
    and_v16b, bic_v16b, bif_v16b, bit_v16b, bsl_v16b, cmeq_v2d, cmge_v2d, cmgt_v2d, dup_2d_d,
    dup_d_d1, ext_v16b_8, fadd_v2d, fcmeq_v2d, fcmge_v2d, fcmgt_v2d, fcvtzs_v2d, fdiv_v2d,
    fmul_v2d, frintz_v2d, fsub_v2d, ins_d1_d0, ldr_q_imm12, ldur_q_imm9, movi_2d_ones, orr_v16b,
    str_q_imm12, stur_q_imm9, sub_v2d, umov_x_d,
};
pub use sys::{mrs_fpcr, msr_fpcr, orr_imm_bit};
