//! `__torajs_arr_push(arr, v)` opened at the call site.
//!
//! A push that lands in spare capacity of an extensible array with no
//! deque head is a slot store and a length bump; the call around it
//! cost more than that and made every value live across the site
//! spill. The site does that case inline and makes the plain call for
//! everything else: a locked receiver (which throws), a head to fold
//! back, a full buffer.

use core::mem::offset_of;

use torajs_arr::grow::{ARR_HDR_CAP_OFF, ARR_HDR_HEAD_OFF, ARR_HDR_LEN_OFF};
use torajs_arr::layout::ARR_DATA_PTR_OFF;
use torajs_core::ssa::{FuncId, Function, Inst, InstKind, Module, Operand};
use torajs_rc::{FLAG_ARR_LENGTH_RO, FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED, HeapHeader};

use super::rc_dec::{Fix, patch, place, store};
use super::{
    ModuleFacts, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP,
    materialize_operand_gpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_imm, add_reg, and_reg, cmp_reg, cond, ldr_w_imm12, ldr_x_imm12, mov_x_reg, movz_imm,
    str_x_imm12, str_x_reg_lsl3,
};
use crate::reg::Reg;
use crate::regalloc::Assignment;
use crate::reloc::Reloc;

const PUSH_NAMES: [&str; 2] = ["__torajs_arr_push", "__torajs_arr_push_non_deque"];

const LEN_OFF: u32 = ARR_HDR_LEN_OFF as u32;
const CAP_OFF: u32 = ARR_HDR_CAP_OFF as u32;
const HEAD_OFF: u32 = ARR_HDR_HEAD_OFF as u32;
const DATA_OFF: u32 = ARR_DATA_PTR_OFF as u32;

const FLAGS_SHIFT: u32 = offset_of!(HeapHeader, flags) as u32 * 8;
const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);
/// The header bits that refuse a new index (§10.1.8.1 and a locked
/// `length`).
const LOCK_BITS: [u16; 4] = [
    FLAG_FROZEN,
    FLAG_SEALED,
    FLAG_NON_EXTENSIBLE,
    FLAG_ARR_LENGTH_RO,
];
const LOCK_MASK: u16 = {
    let mut m = 0;
    let mut i = 0;
    while i < LOCK_BITS.len() {
        m |= LOCK_BITS[i];
        i += 1;
    }
    m
};
// the mask is placed with `movz ..., lsl #48`, the flags' own half-word
const _: () = assert!(FLAGS_SHIFT == 48);

/// The module's typed array push kernels.
#[derive(Debug, Clone, Copy)]
pub struct ArrPush {
    funcs: [Option<FuncId>; 2],
}

impl ArrPush {
    /// `None` leaves every push a plain call: a module that declares
    /// neither kernel, and `TORAJS_ARR_PUSH_INLINE_OFF=1` (bisect
    /// gate).
    pub fn of_module(module: &Module) -> Option<ArrPush> {
        if std::env::var_os("TORAJS_ARR_PUSH_INLINE_OFF").is_some_and(|v| v == "1") {
            return None;
        }
        let find = |name: &str| {
            module
                .funcs
                .iter()
                .position(|f| f.name == name)
                .map(|i| FuncId(i as u32))
        };
        let funcs = PUSH_NAMES.map(find);
        funcs
            .iter()
            .any(Option::is_some)
            .then_some(ArrPush { funcs })
    }

    /// Whether `kind` is a push [`try_emit`] opens.
    pub(crate) fn matches(self, kind: &InstKind) -> bool {
        matches!(kind, InstKind::Call(fid, args)
            if self.funcs.contains(&Some(*fid))
                && matches!(args.as_slice(), [Operand::Value(_), _]))
    }
}

/// The push at `inst`, opened inline; `false` when it is not one.
///
/// ```text
///     ldr   x10, [xA]
///     movz  x11, #lock_bits, lsl #48
///     and   x11, x10, x11 ; cbnz x11, slow  ; four lock bits
///     ldr   w10, [xA, #head] ; ldr x11, [xA, #len]
///     add   x11, x11, x10                 ; physical index
///     ldr   w10, [xA, #cap]
///     cmp   x11, x10 ; b.hs slow
///     ldr   x10, [xA, #data]
///     str   xV, [x10, x11, lsl #3]
///     ldr   x11, [xA, #len]
///     add   x11, x11, #1 ; str x11, [xA, #len]
///     mov   xD, xA ; b done
/// slow:
///     <the plain call, with the call saves around it>
/// done:
/// ```
pub(super) fn try_emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    func: &Function,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
    facts: &ModuleFacts,
) -> bool {
    let Some(op) = facts.inline.arr_push else {
        return false;
    };
    if !op.matches(&inst.kind) {
        return false;
    }
    let InstKind::Call(_, args) = &inst.kind else {
        return false;
    };
    let Some(result) = inst.result else {
        return false;
    };
    // the kernel takes the slot bits in a general register; a value
    // the call would convert keeps the call
    if super::operand::operand_is_f64(&args[1], alloc) {
        return false;
    }
    let arr = materialize_operand_gpr(bytes, &args[0], OP_SCRATCH_LHS, alloc);
    let val = materialize_operand_gpr(bytes, &args[1], OP_SCRATCH_RESULT_GPR, alloc);
    let (hdr, len) = (OP_SCRATCH_RHS, OP_SCRATCH_TMP);
    let mut slow = Vec::new();

    write_u32(bytes, ldr_x_imm12(hdr, arr, 0));
    write_u32(bytes, movz_imm(len, LOCK_MASK, 3));
    write_u32(bytes, and_reg(len, hdr, len));
    slow.push(place(bytes, Fix::Cbnz(len)));
    // `len` holds the physical slot index, head + len, until the store
    write_u32(bytes, ldr_w_imm12(hdr, arr, HEAD_OFF));
    write_u32(bytes, ldr_x_imm12(len, arr, LEN_OFF));
    write_u32(bytes, add_reg(len, len, hdr));
    write_u32(bytes, ldr_w_imm12(hdr, arr, CAP_OFF));
    write_u32(bytes, cmp_reg(len, hdr));
    slow.push(place(bytes, Fix::Cond(cond::HS)));
    write_u32(bytes, ldr_x_imm12(hdr, arr, DATA_OFF));
    write_u32(bytes, str_x_reg_lsl3(val, hdr, len));
    write_u32(bytes, ldr_x_imm12(len, arr, LEN_OFF));
    write_u32(bytes, add_imm(len, len, 1));
    write_u32(bytes, str_x_imm12(len, arr, LEN_OFF));
    let (dst, spill_off) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
    if dst != arr {
        write_u32(bytes, mov_x_reg(dst, arr));
    }
    write_def_spill_gpr(bytes, spill_off, dst);
    let to_done = place(bytes, Fix::B);

    let slow_at = bytes.len();
    for at in slow {
        patch(bytes, at, slow_at);
    }
    for &(reg, off) in saves {
        store(bytes, reg, off, true);
    }
    super::inst::emit_plain(bytes, relocs, func, inst, alloc, &facts.fn_sigs);
    for &(reg, off) in saves {
        store(bytes, reg, off, false);
    }
    let done_at = bytes.len();
    patch(bytes, to_done, done_at);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_bits_sit_in_the_flags_half_word() {
        for bit in LOCK_BITS {
            let b = FLAGS_SHIFT + bit.trailing_zeros();
            assert!((48..64).contains(&b));
        }
    }
}
