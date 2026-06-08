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
//! under the 500-LOC hard limit (see `.claude/rules/common/file-
//! size.md`):
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

mod ctrl;
mod fp;
mod int;
mod mem;

pub use ctrl::{
    adrp, b_cond_imm19, b_imm26, bl_imm26, blr_reg, brk_imm16, cbnz_x, cbz_x, cond, cset_cond, ret,
};
pub use fp::{
    fadd_d, fcmp_d, fcvtzs_x_d, fdiv_d, fmov_d_from_x, fmov_d_to_d, fmov_x_from_d, fmul_d, fsub_d,
    ldr_d_imm12, ldr_d_reg, scvtf_d_x, str_d_imm12, str_d_reg,
};
pub use int::{
    add_imm, add_reg, and_imm_one, and_reg, asrv_reg, cmp_reg, eor_reg, lslv_reg, lsrv_reg,
    mov_w_reg, mov_x_reg, movk_imm, movz_imm, msub_reg, mul_reg, orr_reg, sdiv_reg, sub_imm,
    sub_reg, udiv_reg,
};
pub use mem::{
    ldp_post_index, ldr_x_imm12, ldr_x_reg, ldur_x_imm9, stp_pre_index, str_x_imm12, str_x_reg,
    stur_x_imm9,
};
