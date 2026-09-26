//! Inline refcount release: `InstKind::RcDecAny`.

use core::mem::offset_of;

use torajs_core::ssa::Operand;
use torajs_rc::{FLAG_BUFFERED, HeapHeader, Tag};

use super::rc::{NON_CELL_TOP_SHIFT, STATIC_BIT, TAG_BIT_TYPE_OTHER};
use super::{
    OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_operand_gpr, slot, write_u32,
};
use crate::enc::{
    b_cond_imm19, b_imm26, bl_imm26, cbnz_x, cbz_x, cmp_w_imm, cond, ldr_x_imm12, lsr_imm,
    mov_x_reg, str_w_imm12, sub_w_imm, tbnz, tst_imm,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

const BUFFERED_BIT: u32 = offset_of!(HeapHeader, flags) as u32 * 8 + FLAG_BUFFERED.trailing_zeros();
// torajs-str's FLAG_SUBSTR_INLINE: flags bit 0 on a Str cell, whose
// release drops the parent string rather than the cell itself
const SUBSTR_INLINE_BIT: u32 = offset_of!(HeapHeader, flags) as u32 * 8;
// `tst` of the type_tag half-word (bits 32..48): 16 ones rotated to 32
const TAG_FIELD: (u32, u32, u32) = (1, 32, 15);
const _: () = assert!(offset_of!(HeapHeader, type_tag) == 4);
const _: () = assert!(Tag::Str as u16 == 0);
const DROP_SYM: &str = "___torajs_value_drop_heap";

enum Fix {
    Cbnz(Gpr),
    Cbz(Gpr),
    Tbnz(Gpr, u32),
    Cond(u8),
    B,
}

/// `__torajs_anyv_rc_dec`: release a cell's reference through
/// `__torajs_value_drop_heap`, leave immediates alone. The count is
/// decremented in place only where that call would do nothing more:
///
/// ```text
///     lsr   x10, xV, #48 ; cbnz x10, done     ; numbers, ShortStr
///     tbnz  xV, #1, done ; cbz xV, done       ; sentinels, null
///     ldr   x10, [xV]
///     tbnz  x10, #STATIC_BIT, static
///     cmp   w10, #1 ; b.ls slow               ; last reference
///     tbnz  x10, #BUFFERED_BIT, dec           ; already a cycle root
///     tst   x10, #tag ; b.ne slow             ; may need buffering
///     tbnz  x10, #SUBSTR_INLINE_BIT, slow     ; releases its parent
/// dec:
///     sub   w11, w10, #1 ; str w11, [xV] ; b done
/// static:                                     ; only a Str is inert
///     tst   x10, #tag ; b.eq done
/// slow:
///     <store the caller-saved values live across this site>
///     mov   x0, xV ; bl ___torajs_value_drop_heap
///     <reload them>
/// done:
/// ```
///
/// `saves` are the allocator's call saves for this site: they belong
/// to the call, so only the slow arm pays them.
pub fn emit_rc_dec_any(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    v: &Operand,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
) {
    let Operand::Value(_) = v else {
        // constants are immediates or null: nothing to release
        return;
    };
    let p = materialize_operand_gpr(bytes, v, OP_SCRATCH_LHS, alloc);
    let hdr = OP_SCRATCH_RHS;
    let mut done = Vec::new();
    let mut slow = Vec::new();

    write_u32(bytes, lsr_imm(hdr, p, NON_CELL_TOP_SHIFT));
    done.push(place(bytes, Fix::Cbnz(hdr)));
    done.push(place(bytes, Fix::Tbnz(p, TAG_BIT_TYPE_OTHER)));
    done.push(place(bytes, Fix::Cbz(p)));
    write_u32(bytes, ldr_x_imm12(hdr, p, 0));
    let to_static = place(bytes, Fix::Tbnz(hdr, STATIC_BIT));
    write_u32(bytes, cmp_w_imm(hdr, 1));
    slow.push(place(bytes, Fix::Cond(cond::LS)));
    let to_dec = place(bytes, Fix::Tbnz(hdr, BUFFERED_BIT));
    write_u32(bytes, tst_imm(hdr, TAG_FIELD.0, TAG_FIELD.1, TAG_FIELD.2));
    slow.push(place(bytes, Fix::Cond(cond::NE)));
    slow.push(place(bytes, Fix::Tbnz(hdr, SUBSTR_INLINE_BIT)));

    let dec_at = bytes.len();
    patch(bytes, to_dec, dec_at);
    write_u32(bytes, sub_w_imm(OP_SCRATCH_TMP, hdr, 1));
    write_u32(bytes, str_w_imm12(OP_SCRATCH_TMP, p, 0));
    done.push(place(bytes, Fix::B));

    let static_at = bytes.len();
    patch(bytes, to_static, static_at);
    write_u32(bytes, tst_imm(hdr, TAG_FIELD.0, TAG_FIELD.1, TAG_FIELD.2));
    done.push(place(bytes, Fix::Cond(cond::EQ)));

    let slow_at = bytes.len();
    for &(reg, off) in saves {
        store(bytes, reg, off, true);
    }
    if p != Gpr::X0 {
        write_u32(bytes, mov_x_reg(Gpr::X0, p));
    }
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::CallSite {
            target: CallTarget::Extern(DROP_SYM.into()),
        },
    });
    write_u32(bytes, bl_imm26(0));
    for &(reg, off) in saves {
        store(bytes, reg, off, false);
    }

    let done_at = bytes.len();
    for at in slow {
        patch(bytes, at, slow_at);
    }
    for at in done {
        patch(bytes, at, done_at);
    }
}

fn store(bytes: &mut Vec<u8>, reg: Reg, off: u32, save: bool) {
    match (reg, save) {
        (Reg::Gpr(g), true) => slot::sp_str_x(bytes, g, off),
        (Reg::Gpr(g), false) => slot::sp_ldr_x(bytes, g, off),
        (Reg::Fpr(f), true) => slot::sp_str_d(bytes, f, off),
        (Reg::Fpr(f), false) => slot::sp_ldr_d(bytes, f, off),
        (other, _) => unreachable!("saved value not in a register: {other:?}"),
    }
}

/// Reserve a branch word; [`patch`] encodes it once the target is known.
fn place(bytes: &mut Vec<u8>, fix: Fix) -> (usize, Fix) {
    let at = bytes.len();
    write_u32(bytes, 0);
    (at, fix)
}

fn patch(bytes: &mut [u8], (at, fix): (usize, Fix), target: usize) {
    let off = (target - at) as i32;
    let word = match fix {
        Fix::Cbnz(r) => cbnz_x(r, off),
        Fix::Cbz(r) => cbz_x(r, off),
        Fix::Tbnz(r, bit) => tbnz(r, bit, off),
        Fix::Cond(c) => b_cond_imm19(c, off),
        Fix::B => b_imm26(off),
    };
    bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_bits() {
        assert_eq!(BUFFERED_BIT, 53);
        assert_eq!(SUBSTR_INLINE_BIT, 48);
        assert_eq!(STATIC_BIT, 50);
    }
}
