//! `acc = acc + <short literal>` written into the accumulator's slack
//! at the call site: `Call(__torajs_str_append_lit, [acc, bits, meta])`
//! with both literal immediates constant.
//!
//! The kernel's in-place shortcut is a handful of loads, one or two
//! stores and a new length, but reaching it through a call costs more
//! than the work. The site keeps the one case that dominates a
//! growing string — a cell this reference alone holds, already grown
//! (so its capacity sits in the header), of the literal's width, with
//! room left — and hands everything else to the kernel unchanged.

use core::mem::offset_of;

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};
use torajs_rc::HeapHeader;
use torajs_str::concat_lit::{LIT_META_LEN_MASK, LIT_META_LEN_SHIFT};
use torajs_str::layout::{
    STR_DATA_OFF, STR_FLAG_HAS_CAPACITY, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF,
    STR_PAD_OFF,
};

use super::rc_dec::{Fix, patch, place, store};
use super::{
    OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_const_i64,
    materialize_operand_gpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_imm, add_reg_lsl, add_w_imm, and_imm, bl_imm26, cmp_imm, cmp_reg, cmp_w_imm, cond,
    ldr_w_imm12, ldr_x_imm12, logical_imm, lsl_imm, lsr_imm, mov_x_reg, str_w_imm12, strb_w_imm12,
    stur_w_imm9, stur_x_imm9, sturh_w_imm9,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

const APPEND_LIT_NAME: &str = "__torajs_str_append_lit";

// the header is read as one little-endian word: count, tag, flags; the
// tag is not compared, the kernel's contract already makes it a Str
const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);
const _: () = assert!(offset_of!(HeapHeader, type_tag) == 4);
const FLAGS_SHIFT: u32 = offset_of!(HeapHeader, flags) as u32 * 8;
// the expected flags are compared as a 12-bit immediate
const _: () = assert!(STR_FLAG_HAS_CAPACITY < 4096 && STR_FLAG_IS_LATIN1 < 4096);
// the two header words the site reads sit side by side
const _: () = assert!(STR_PAD_OFF == STR_LEN_OFF + 4);
// a one-byte store reaches the data with an unscaled 12-bit offset,
// every wider one with a 9-bit signed one
const _: () = assert!(STR_DATA_OFF + 8 <= 255);

/// The module's consuming short-literal append.
#[derive(Debug, Clone, Copy)]
pub struct StrAppendLit {
    func: FuncId,
}

/// What the site needs of the literal, decoded from its immediates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lit {
    bits: u64,
    meta: u64,
    units: u32,
    latin1: bool,
    ascii: bool,
}

impl Lit {
    fn bytes(self) -> u32 {
        self.units << u32::from(!self.latin1)
    }

    /// The flags word a cell must carry, once an ASCII literal has
    /// masked the cell's own ASCII mark out of it: grown, of the
    /// literal's width, and nothing else — no `.rodata` literal, no
    /// view, no inline substring.
    fn expected_flags(self) -> u16 {
        STR_FLAG_HAS_CAPACITY | if self.latin1 { STR_FLAG_IS_LATIN1 } else { 0 }
    }
}

impl StrAppendLit {
    /// `None` leaves every append a plain call: a module that never
    /// declares the kernel, and `TORAJS_STR_APPEND_INLINE_OFF=1`
    /// (bisect gate).
    pub fn of_module(module: &Module) -> Option<StrAppendLit> {
        if std::env::var_os("TORAJS_STR_APPEND_INLINE_OFF").is_some_and(|v| v == "1") {
            return None;
        }
        let at = module
            .funcs
            .iter()
            .position(|f| f.name == APPEND_LIT_NAME)?;
        Some(StrAppendLit {
            func: FuncId(at as u32),
        })
    }

    /// The literal of a call [`try_emit`] opens inline.
    pub(crate) fn literal(self, kind: &InstKind) -> Option<Lit> {
        let InstKind::Call(fid, args) = kind else {
            return None;
        };
        if *fid != self.func {
            return None;
        }
        let [
            Operand::Value(_),
            Operand::ConstI64(bits),
            Operand::ConstI64(meta),
        ] = args.as_slice()
        else {
            return None;
        };
        let (bits, meta) = (*bits as u64, *meta as u64);
        let flags = meta as u16;
        let lit = Lit {
            bits,
            meta,
            units: (meta >> LIT_META_LEN_SHIFT) as u32 & LIT_META_LEN_MASK,
            latin1: flags & STR_FLAG_IS_LATIN1 != 0,
            ascii: flags & STR_FLAG_IS_ASCII != 0,
        };
        (lit.units != 0 && lit.bytes() <= 8).then_some(lit)
    }
}

/// The append at `inst`, opened inline; `false` when it is not one.
///
/// ```text
///     cbz   xA, slow
///     ldr   x10, [xA]                     ; count | tag | flags
///     cmp   w10, #1 ; b.ne slow           ; someone else holds it
///     lsr   x10, x10, #48
///     and   x10, x10, #~IS_ASCII          ; ASCII literal only
///     cmp   x10, #flags ; b.ne slow       ; not grown / other width / static / view
///     ldr   w10, [xA, #len] ; ldr w11, [xA, #cap]
///     lsl   x12, x10, #1                  ; UTF-16 only
///     add   x12, x12, #bytes
///     cmp   x12, x11 ; b.hi slow          ; out of room
///     add   x11, xA, x10, lsl #shift
///     <literal bytes into [x11, #data]>
///     add   w10, w10, #units ; str w10, [xA, #len]
///     mov   xD, xA ; b done
/// slow:
///     <store the call saves>
///     mov x0, xA ; mov x1, #bits ; mov x2, #meta ; bl append_lit
///     mov xD, x0
///     <reload them>
/// done:
/// ```
///
/// A cell marked ASCII meets a non-ASCII literal on the slow arm, so
/// the inline arm never has to clear the mark. Only the slow arm pays
/// `saves`, the allocator's call saves for this site.
pub(super) fn try_emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
    op: StrAppendLit,
) -> bool {
    let Some(lit) = op.literal(&inst.kind) else {
        return false;
    };
    let InstKind::Call(fid, args) = &inst.kind else {
        return false;
    };
    let result = inst.result.expect("str_append_lit must have a result");
    let (len, cap, need) = (OP_SCRATCH_RHS, OP_SCRATCH_TMP, OP_SCRATCH_RESULT_GPR);
    let acc = materialize_operand_gpr(bytes, &args[0], OP_SCRATCH_LHS, alloc);
    let mut slow = Vec::new();

    slow.push(place(bytes, Fix::Cbz(acc)));
    write_u32(bytes, ldr_x_imm12(len, acc, 0));
    write_u32(bytes, cmp_w_imm(len, 1));
    slow.push(place(bytes, Fix::Cond(cond::NE)));
    write_u32(bytes, lsr_imm(len, len, FLAGS_SHIFT));
    if lit.ascii {
        let keep = logical_imm(!u64::from(STR_FLAG_IS_ASCII)).expect("one cleared bit encodes");
        write_u32(bytes, and_imm(len, len, keep));
    }
    write_u32(bytes, cmp_imm(len, lit.expected_flags()));
    slow.push(place(bytes, Fix::Cond(cond::NE)));

    let shift = u32::from(!lit.latin1);
    write_u32(bytes, ldr_w_imm12(len, acc, STR_LEN_OFF as u32));
    write_u32(bytes, ldr_w_imm12(cap, acc, STR_PAD_OFF as u32));
    let used = if shift == 0 {
        len
    } else {
        write_u32(bytes, lsl_imm(need, len, shift));
        need
    };
    write_u32(bytes, add_imm(need, used, lit.bytes() as u16));
    write_u32(bytes, cmp_reg(need, cap));
    slow.push(place(bytes, Fix::Cond(cond::HI)));
    write_u32(bytes, add_reg_lsl(cap, acc, len, shift));
    store_literal(bytes, cap, need, lit);
    write_u32(bytes, add_w_imm(len, len, lit.units as u16));
    write_u32(bytes, str_w_imm12(len, acc, STR_LEN_OFF as u32));

    let (dst, spill_off) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
    if dst != acc {
        write_u32(bytes, mov_x_reg(dst, acc));
    }
    let to_done = place(bytes, Fix::B);

    let slow_at = bytes.len();
    for at in slow {
        patch(bytes, at, slow_at);
    }
    for &(reg, off) in saves {
        store(bytes, reg, off, true);
    }
    if acc != Gpr::X0 {
        write_u32(bytes, mov_x_reg(Gpr::X0, acc));
    }
    materialize_const_i64(bytes, Gpr::X1, lit.bits as i64);
    materialize_const_i64(bytes, Gpr::X2, lit.meta as i64);
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::CallSite {
            target: CallTarget::Func(*fid),
        },
    });
    write_u32(bytes, bl_imm26(0));
    // before the reload: a saved register may be x0 itself
    if dst != Gpr::X0 {
        write_u32(bytes, mov_x_reg(dst, Gpr::X0));
    }
    for &(reg, off) in saves {
        store(bytes, reg, off, false);
    }

    let done_at = bytes.len();
    patch(bytes, to_done, done_at);
    write_def_spill_gpr(bytes, spill_off, dst);
    true
}

/// The literal's bytes at `[at, #STR_DATA_OFF]`, staged through `val`:
/// one store for a power-of-two length, two overlapping ones between.
fn store_literal(bytes: &mut Vec<u8>, at: Gpr, val: Gpr, lit: Lit) {
    let n = lit.bytes() as i32;
    let off = STR_DATA_OFF as i32;
    materialize_const_i64(bytes, val, lit.bits as i64);
    match n {
        1 => write_u32(bytes, strb_w_imm12(val, at, off as u32)),
        2 => write_u32(bytes, sturh_w_imm9(val, at, off)),
        3 => {
            write_u32(bytes, sturh_w_imm9(val, at, off));
            write_u32(bytes, lsr_imm(val, val, 8));
            write_u32(bytes, sturh_w_imm9(val, at, off + 1));
        }
        4 => write_u32(bytes, stur_w_imm9(val, at, off)),
        5..=7 => {
            write_u32(bytes, stur_w_imm9(val, at, off));
            write_u32(bytes, lsr_imm(val, val, (n as u32 - 4) * 8));
            write_u32(bytes, stur_w_imm9(val, at, off + n - 4));
        }
        8 => write_u32(bytes, stur_x_imm9(val, at, off)),
        _ => unreachable!("literal() admits one to eight bytes"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use torajs_core::ssa::ValueId;

    use super::*;
    use crate::compile::slot;
    use crate::enc::{b_cond_imm19, b_imm26, cbz_x, movk_imm, movz_imm};

    const OP: StrAppendLit = StrAppendLit { func: FuncId(7) };

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn meta(units: u64, flags: u16) -> i64 {
        ((units << LIT_META_LEN_SHIFT) | u64::from(flags)) as i64
    }

    fn call(args: Vec<Operand>) -> Inst {
        Inst {
            result: Some(ValueId(1)),
            kind: InstKind::Call(OP.func, args),
            origin: None,
        }
    }

    fn acc() -> Operand {
        Operand::Value(ValueId(0))
    }

    #[test]
    fn only_constant_literal_appends_open_inline() {
        let ascii = meta(1, STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII);
        let lit = |args| OP.literal(&call(args).kind);
        assert!(
            lit(vec![
                acc(),
                Operand::ConstI64(0x2c),
                Operand::ConstI64(ascii)
            ])
            .is_some()
        );
        // a null or constant accumulator, a literal known only at run time
        assert!(
            lit(vec![
                Operand::ConstPtrNull,
                Operand::ConstI64(1),
                Operand::ConstI64(ascii)
            ])
            .is_none()
        );
        assert!(lit(vec![acc(), acc(), Operand::ConstI64(ascii)]).is_none());
        // another callee
        let other = Inst {
            result: Some(ValueId(1)),
            kind: InstKind::Call(
                FuncId(8),
                vec![acc(), Operand::ConstI64(0x2c), Operand::ConstI64(ascii)],
            ),
            origin: None,
        };
        assert!(OP.literal(&other.kind).is_none());
        // four UTF-16 units are eight bytes; five would not travel by value
        assert_eq!(
            lit(vec![
                acc(),
                Operand::ConstI64(0),
                Operand::ConstI64(meta(4, 0))
            ])
            .map(Lit::bytes),
            Some(8)
        );
        assert!(
            lit(vec![
                acc(),
                Operand::ConstI64(0),
                Operand::ConstI64(meta(5, 0))
            ])
            .is_none()
        );
    }

    #[test]
    fn the_inline_arm_is_the_kernels_in_place_case() {
        let alloc = Assignment::from_parts(
            [(0, Reg::Gpr(Gpr::X19)), (1, Reg::Gpr(Gpr::X19))].into(),
            HashMap::new(),
            0,
            32,
            true,
        );
        // "中文" onto a UTF-16 accumulator
        let bits = 0x6587_4E2D_i64;
        let m = meta(2, 0);
        let inst = call(vec![acc(), Operand::ConstI64(bits), Operand::ConstI64(m)]);
        let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
        let saves = [(Reg::Gpr(Gpr::X6), 24)];
        assert!(try_emit(&mut bytes, &mut relocs, &inst, &alloc, &saves, OP));

        let a = Gpr::X19;
        let (len, cap, need) = (Gpr::X10, Gpr::X11, Gpr::X12);
        let inline = [
            cbz_x(a, 20 * 4),
            ldr_x_imm12(len, a, 0),
            cmp_w_imm(len, 1),
            b_cond_imm19(cond::NE, 17 * 4),
            lsr_imm(len, len, 48),
            cmp_imm(len, STR_FLAG_HAS_CAPACITY),
            b_cond_imm19(cond::NE, 14 * 4),
            ldr_w_imm12(len, a, 8),
            ldr_w_imm12(cap, a, 12),
            lsl_imm(need, len, 1),
            add_imm(need, need, 4),
            cmp_reg(need, cap),
            b_cond_imm19(cond::HI, 8 * 4),
            add_reg_lsl(cap, a, len, 1),
            movz_imm(need, 0x4E2D, 0),
            movk_imm(need, 0x6587, 1),
            stur_w_imm9(need, cap, 16),
            add_w_imm(len, len, 2),
            str_w_imm12(len, a, 8),
        ];
        let mut expect: Vec<u8> = Vec::new();
        for w in inline {
            expect.extend_from_slice(&w.to_le_bytes());
        }
        expect.extend_from_slice(&b_imm26(9 * 4).to_le_bytes());
        slot::sp_str_x(&mut expect, Gpr::X6, 24);
        for w in [
            mov_x_reg(Gpr::X0, a),
            movz_imm(Gpr::X1, 0x4E2D, 0),
            movk_imm(Gpr::X1, 0x6587, 1),
            movz_imm(Gpr::X2, m as u16, 0),
            bl_imm26(0),
            mov_x_reg(a, Gpr::X0),
        ] {
            expect.extend_from_slice(&w.to_le_bytes());
        }
        slot::sp_ldr_x(&mut expect, Gpr::X6, 24);
        assert_eq!(words(&bytes), words(&expect));
        assert_eq!(relocs.len(), 1);
        assert_eq!(relocs[0].byte_offset, 25 * 4);
    }

    #[test]
    fn an_ascii_literal_masks_the_cells_mark_and_stores_by_length() {
        let alloc = Assignment::from_parts(
            [(0, Reg::Gpr(Gpr::X3)), (1, Reg::Gpr(Gpr::X4))].into(),
            HashMap::new(),
            0,
            0,
            true,
        );
        for n in 1..=8u64 {
            let bits = 0x3837_3635_3433_3231_u64 & (u64::MAX >> (64 - 8 * n));
            let m = meta(n, STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII);
            let inst = call(vec![
                acc(),
                Operand::ConstI64(bits as i64),
                Operand::ConstI64(m),
            ]);
            let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
            assert!(try_emit(&mut bytes, &mut relocs, &inst, &alloc, &[], OP));
            let w = words(&bytes);
            let keep = logical_imm(!u64::from(STR_FLAG_IS_ASCII)).unwrap();
            assert_eq!(w[5], and_imm(Gpr::X10, Gpr::X10, keep), "n = {n}");
            assert_eq!(
                w[6],
                cmp_imm(Gpr::X10, STR_FLAG_HAS_CAPACITY | STR_FLAG_IS_LATIN1)
            );
            // Latin-1: the used bytes are the length itself
            assert_eq!(w[10], add_imm(Gpr::X12, Gpr::X10, n as u16), "n = {n}");
            assert_eq!(w[13], add_reg_lsl(Gpr::X11, Gpr::X3, Gpr::X10, 0));
        }
    }
}
