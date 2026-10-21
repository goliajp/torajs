//! `acc = acc + view` written into the accumulator's slack at the call
//! site: `Call(__torajs_str_append_substr, [acc, view])`, the shape a
//! string builder takes when it appends a slice or a split part.
//!
//! The kernel's in-place case is a few header reads and a short copy;
//! through a call it also pays the call, the caller-saved registers
//! stored and reloaded around it, and the kernel's dispatch on width
//! and copy length. The site keeps the case that dominates a builder
//! loop — a Latin-1 cell this reference alone holds, already grown (its
//! capacity in the header), a Latin-1 view of another string, at most
//! 16 bytes, with room left — and hands everything else to the kernel
//! unchanged.

use core::mem::offset_of;

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};
use torajs_rc::HeapHeader;
use torajs_str::layout::{
    STR_DATA_OFF, STR_FLAG_HAS_CAPACITY, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF,
    STR_PAD_OFF,
};
use torajs_str::substr::{SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF};

use crate::compile::operand::access_base;
use crate::compile::rc_dec::{Fix, patch, place, store};
use crate::compile::slot::X_IMM12_MAX;
use crate::compile::{
    OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_operand_gpr,
    write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_imm, add_reg, and_imm, and_reg, bl_imm26, cmp_imm, cmp_reg, cmp_w_imm, cond, ldr_w_imm12,
    ldr_x_imm12, ldrb_w_imm12, ldrb_w_reg, ldur_w_imm9, ldur_x_imm9, logical_imm, lsr_imm,
    mov_x_reg, orr_imm, str_w_imm12, str_x_imm12, strb_w_imm12, stur_w_imm9, stur_x_imm9,
    sturh_w_imm9, sub_imm, sub_reg, tst_imm,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

const APPEND_SUBSTR_NAME: &str = "__torajs_str_append_substr";

/// Longest view the site copies; longer ones take the kernel's copy.
const MAX_INLINE_BYTES: u16 = 16;

// the header is read as one little-endian word: count, tag, flags
const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);
const FLAGS_OFF: i32 = offset_of!(HeapHeader, flags) as i32;
const FLAGS_SHIFT: u32 = FLAGS_OFF as u32 * 8;
const _: () = assert!(STR_FLAG_HAS_CAPACITY < 4096 && STR_FLAG_IS_LATIN1 < 4096);
// the copy addresses the payload with unscaled offsets past the base
const _: () = assert!(STR_DATA_OFF == 16);

/// The largest frame offset at which a spilled view folds into the
/// field loads `[sp, #slot + field]`: past it the furthest field leaves
/// the scaled imm12's reach and the view's address takes an ADD.
pub(crate) const VIEW_REACH: u32 = X_IMM12_MAX - max_field_off();

const fn max_field_off() -> u32 {
    let (a, b, c) = (SUBSTR_PARENT_OFF, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF);
    let ab = if a > b { a } else { b };
    (if ab > c { ab } else { c }) as u32
}

/// The module's consuming substring append.
#[derive(Debug, Clone, Copy)]
pub struct StrAppendSubstr {
    func: FuncId,
}

impl StrAppendSubstr {
    /// `None` leaves every substring append a plain call: a module that
    /// never declares the kernel, and `TORAJS_STR_APPEND_INLINE_OFF=1`
    /// (bisect gate, shared with the literal append).
    pub fn of_module(module: &Module) -> Option<StrAppendSubstr> {
        if std::env::var_os("TORAJS_STR_APPEND_INLINE_OFF").is_some_and(|v| v == "1") {
            return None;
        }
        let at = module
            .funcs
            .iter()
            .position(|f| f.name == APPEND_SUBSTR_NAME)?;
        Some(StrAppendSubstr {
            func: FuncId(at as u32),
        })
    }

    /// Whether [`try_emit`] opens the call `kind` inline.
    pub(crate) fn matches(self, kind: &InstKind) -> bool {
        matches!(
            kind,
            InstKind::Call(fid, args)
                if *fid == self.func
                    && matches!(args.as_slice(), [Operand::Value(_), Operand::Value(_)])
        )
    }
}

/// The append at `inst`, opened inline; `false` when it is not one.
///
/// ```text
///     cbz   xA, slow
///     ldr   x11, [xV, #parent] ; ldr x12, [xV, #len] ; ldr x16, [xV, #offset]
///     ldr   x10, [xA] ; cmp w10, #1 ; b.ne slow       ; someone else holds it
///     lsr   x10, x10, #48                             ; the cell's flags
///     and   x17, x10, #~IS_ASCII
///     cmp   x17, #(HAS_CAPACITY|IS_LATIN1) ; b.ne slow ; not grown / UTF-16 / static / view
///     cmp   x11, xA ; b.eq slow                       ; a view of the cell itself
///     cmp   x12, #16 ; b.hi slow                      ; a long view
///     ldr   x17, [x11] ; lsr x17, x17, #48
///     tst   x17, #IS_LATIN1 ; b.eq slow               ; a UTF-16 parent
///     orr   x17, x17, #~IS_ASCII ; and x10, x10, x17  ; the cell stays ASCII only if the parent is
///     add   x11, x11, x16                             ; the view's bytes, past the header
///     ldr   w16, [xA, #len] ; ldr w17, [xA, #cap]
///     sub   x17, x17, x16 ; cmp x12, x17 ; b.hi slow  ; out of room
///     add   x17, x16, x12 ; str w17, [xA, #len]
///     strh  w10, [xA, #flags]
///     add   x16, xA, x16                              ; the end of the cell's bytes
///     <copy x12 bytes, 0..=16, from [x11, #16] to [x16, #16]>
///     mov   xD, xA ; b done
/// slow:
///     <store the call saves>
///     mov x17, xV ; mov x0, xA ; mov x1, x17 ; bl append_substr
///     mov xD, x0
///     <reload them>
/// done:
/// ```
///
/// Only the slow arm pays `saves`, the allocator's call saves for this
/// site. Nothing is written before the last check passes, so the slow
/// arm meets the cell as it was.
pub(crate) fn try_emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
    op: StrAppendSubstr,
) -> bool {
    if !op.matches(&inst.kind) {
        return false;
    }
    let InstKind::Call(fid, args) = &inst.kind else {
        return false;
    };
    let result = inst.result.expect("str_append_substr must have a result");
    let (flags, parent, n) = (OP_SCRATCH_RHS, OP_SCRATCH_TMP, OP_SCRATCH_RESULT_GPR);
    let (t0, t1) = (Gpr::X16, Gpr::X17);
    let acc = materialize_operand_gpr(bytes, &args[0], OP_SCRATCH_LHS, alloc);
    // a view in a spilled frame slot is read off SP
    let (view, at) = access_base(bytes, &args[1], t1, alloc, VIEW_REACH);
    let at = at as u32;
    let mut slow = Vec::new();

    slow.push(place(bytes, Fix::Cbz(acc)));
    write_u32(
        bytes,
        ldr_x_imm12(parent, view, at + SUBSTR_PARENT_OFF as u32),
    );
    write_u32(bytes, ldr_x_imm12(n, view, at + SUBSTR_LEN_OFF as u32));
    write_u32(bytes, ldr_x_imm12(t0, view, at + SUBSTR_OFFSET_OFF as u32));
    write_u32(bytes, ldr_x_imm12(flags, acc, 0));
    write_u32(bytes, cmp_w_imm(flags, 1));
    slow.push(place(bytes, Fix::Cond(cond::NE)));
    write_u32(bytes, lsr_imm(flags, flags, FLAGS_SHIFT));
    let not_ascii = logical_imm(!u64::from(STR_FLAG_IS_ASCII)).expect("one cleared bit encodes");
    write_u32(bytes, and_imm(t1, flags, not_ascii));
    write_u32(
        bytes,
        cmp_imm(t1, STR_FLAG_HAS_CAPACITY | STR_FLAG_IS_LATIN1),
    );
    slow.push(place(bytes, Fix::Cond(cond::NE)));
    write_u32(bytes, cmp_reg(parent, acc));
    slow.push(place(bytes, Fix::Cond(cond::EQ)));
    write_u32(bytes, cmp_imm(n, MAX_INLINE_BYTES));
    slow.push(place(bytes, Fix::Cond(cond::HI)));
    write_u32(bytes, ldr_x_imm12(t1, parent, 0));
    write_u32(bytes, lsr_imm(t1, t1, FLAGS_SHIFT));
    let (tn, tr, ts) = logical_imm(u64::from(STR_FLAG_IS_LATIN1)).expect("one bit encodes");
    write_u32(bytes, tst_imm(t1, tn, tr, ts));
    slow.push(place(bytes, Fix::Cond(cond::EQ)));
    write_u32(bytes, orr_imm(t1, t1, not_ascii));
    write_u32(bytes, and_reg(flags, flags, t1));
    write_u32(bytes, add_reg(parent, parent, t0));
    write_u32(bytes, ldr_w_imm12(t0, acc, STR_LEN_OFF as u32));
    write_u32(bytes, ldr_w_imm12(t1, acc, STR_PAD_OFF as u32));
    write_u32(bytes, sub_reg(t1, t1, t0));
    write_u32(bytes, cmp_reg(n, t1));
    slow.push(place(bytes, Fix::Cond(cond::HI)));
    write_u32(bytes, add_reg(t1, t0, n));
    write_u32(bytes, str_w_imm12(t1, acc, STR_LEN_OFF as u32));
    write_u32(bytes, sturh_w_imm9(flags, acc, FLAGS_OFF));
    write_u32(bytes, add_reg(t0, acc, t0));
    copy_short(bytes, parent, t0, n, flags, t1);

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
    // the view's home may be x0, and a spilled view's scratch was
    // reused above: stage it in x17 before x0 is written
    let view = materialize_operand_gpr(bytes, &args[1], t1, alloc);
    if view != t1 {
        write_u32(bytes, mov_x_reg(t1, view));
    }
    if acc != Gpr::X0 {
        write_u32(bytes, mov_x_reg(Gpr::X0, acc));
    }
    write_u32(bytes, mov_x_reg(Gpr::X1, t1));
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

/// `n` (at most 16) bytes from `[src, #16]` to `[dst, #16]`, through
/// `a` and `b`: two overlapping words of 8 or of 4, or for one to
/// three bytes the first, the middle and the last.
fn copy_short(bytes: &mut Vec<u8>, src: Gpr, dst: Gpr, n: Gpr, a: Gpr, b: Gpr) {
    let off = STR_DATA_OFF as u32;
    let mut done = Vec::new();
    write_u32(bytes, cmp_imm(n, 8));
    let to_eight = place(bytes, Fix::Cond(cond::HS));
    write_u32(bytes, cmp_imm(n, 4));
    let to_four = place(bytes, Fix::Cond(cond::HS));
    done.push(place(bytes, Fix::Cbz(n)));
    write_u32(bytes, ldrb_w_imm12(a, src, off));
    write_u32(bytes, strb_w_imm12(a, dst, off));
    write_u32(bytes, add_imm(src, src, off as u16));
    write_u32(bytes, add_imm(dst, dst, off as u16));
    write_u32(bytes, lsr_imm(b, n, 1));
    write_u32(bytes, ldrb_w_reg(a, src, b));
    write_u32(bytes, add_reg(b, dst, b));
    write_u32(bytes, strb_w_imm12(a, b, 0));
    write_u32(bytes, sub_imm(b, n, 1));
    write_u32(bytes, ldrb_w_reg(a, src, b));
    write_u32(bytes, add_reg(b, dst, b));
    write_u32(bytes, strb_w_imm12(a, b, 0));
    done.push(place(bytes, Fix::B));

    let four_at = bytes.len();
    patch(bytes, to_four, four_at);
    write_u32(bytes, ldr_w_imm12(a, src, off));
    write_u32(bytes, add_reg(b, src, n));
    write_u32(bytes, ldur_w_imm9(b, b, off as i32 - 4));
    write_u32(bytes, str_w_imm12(a, dst, off));
    write_u32(bytes, add_reg(a, dst, n));
    write_u32(bytes, stur_w_imm9(b, a, off as i32 - 4));
    done.push(place(bytes, Fix::B));

    let eight_at = bytes.len();
    patch(bytes, to_eight, eight_at);
    write_u32(bytes, ldr_x_imm12(a, src, off));
    write_u32(bytes, add_reg(b, src, n));
    write_u32(bytes, ldur_x_imm9(b, b, off as i32 - 8));
    write_u32(bytes, str_x_imm12(a, dst, off));
    write_u32(bytes, add_reg(a, dst, n));
    write_u32(bytes, stur_x_imm9(b, a, off as i32 - 8));

    let done_at = bytes.len();
    for at in done {
        patch(bytes, at, done_at);
    }
}

#[cfg(test)]
mod tests {
    use torajs_core::ssa::ValueId;
    use torajs_core::ssa::id_hash::HashMap;

    use super::*;
    use crate::compile::slot;
    use crate::enc::cbz_x;

    const OP: StrAppendSubstr = StrAppendSubstr { func: FuncId(7) };

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn call(fid: FuncId, args: Vec<Operand>) -> Inst {
        Inst {
            result: Some(ValueId(2)),
            kind: InstKind::Call(fid, args),
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn alloc(homes: [(u32, Gpr); 3]) -> Assignment {
        Assignment::from_parts(
            HashMap::from_iter(homes.map(|(v, g)| (v, Reg::Gpr(g)))),
            HashMap::default(),
            0,
            32,
            true,
        )
    }

    #[test]
    fn only_the_kernel_on_two_values_opens_inline() {
        assert!(OP.matches(&call(OP.func, vec![v(0), v(1)]).kind));
        assert!(!OP.matches(&call(FuncId(8), vec![v(0), v(1)]).kind));
        assert!(!OP.matches(&call(OP.func, vec![Operand::ConstPtrNull, v(1)]).kind));
        assert!(!OP.matches(&call(OP.func, vec![v(0)]).kind));
    }

    /// The inline arm reads the view and the cell and writes nothing
    /// before its last check; only the slow arm stores the saves and
    /// calls.
    #[test]
    fn checks_come_before_writes_and_the_call_is_on_the_slow_arm() {
        let a = alloc([(0, Gpr::X19), (1, Gpr::X27), (2, Gpr::X20)]);
        let inst = call(OP.func, vec![v(0), v(1)]);
        let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
        let saves = [(Reg::Gpr(Gpr::X6), 24)];
        assert!(try_emit(&mut bytes, &mut relocs, &inst, &a, &saves, OP));
        let w = words(&bytes);
        let slow = relocs[0].byte_offset as usize / 4 - 4;
        assert_eq!(w[0], cbz_x(Gpr::X19, (slow * 4) as i32));
        assert_eq!(w[1], ldr_x_imm12(Gpr::X11, Gpr::X27, 16));
        assert_eq!(w[2], ldr_x_imm12(Gpr::X12, Gpr::X27, 8));
        assert_eq!(w[3], ldr_x_imm12(Gpr::X16, Gpr::X27, 24));
        let first_store = w
            .iter()
            .position(|&x| x == str_w_imm12(Gpr::X17, Gpr::X19, STR_LEN_OFF as u32))
            .expect("the new length is stored");
        let room = w
            .iter()
            .position(|&x| x == cmp_reg(Gpr::X12, Gpr::X17))
            .expect("the room check");
        assert!(room < first_store);
        assert_eq!(relocs.len(), 1);
        let bl = relocs[0].byte_offset as usize / 4;
        let mut save = Vec::new();
        slot::sp_str_x(&mut save, Gpr::X6, 24);
        assert_eq!(words(&save), [w[slow]], "the slow arm starts with the save");
        assert!(first_store < slow);
        assert_eq!(
            &w[bl - 3..=bl + 1],
            &[
                mov_x_reg(Gpr::X17, Gpr::X27),
                mov_x_reg(Gpr::X0, Gpr::X19),
                mov_x_reg(Gpr::X1, Gpr::X17),
                bl_imm26(0),
                mov_x_reg(Gpr::X20, Gpr::X0),
            ]
        );
    }

    /// The inline arm's words for a view spilled at frame offset `slot`.
    fn spilled_view_words(slot: u32) -> Vec<u32> {
        let a = Assignment::from_parts(
            HashMap::from_iter([
                (0, Reg::Gpr(Gpr::X19)),
                (1, Reg::SpillGpr(slot)),
                (2, Reg::Gpr(Gpr::X20)),
            ]),
            HashMap::from_iter([(1, slot)]),
            slot + 32,
            0,
            true,
        );
        let inst = call(OP.func, vec![v(0), v(1)]);
        let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
        assert!(try_emit(&mut bytes, &mut relocs, &inst, &a, &[], OP));
        words(&bytes)
    }

    /// A spilled view at the edge of the reach is read off SP; one
    /// word past it, its address is added into x17 first, and no field
    /// load carries an offset the scaled imm12 cannot encode.
    #[test]
    fn a_spilled_view_folds_only_within_the_scaled_reach() {
        let w = spilled_view_words(VIEW_REACH);
        assert_eq!(
            &w[1..4],
            &[
                ldr_x_imm12(Gpr::X11, Gpr::SP, VIEW_REACH + 16),
                ldr_x_imm12(Gpr::X12, Gpr::SP, VIEW_REACH + 8),
                ldr_x_imm12(Gpr::X16, Gpr::SP, VIEW_REACH + 24),
            ]
        );
        assert_eq!(VIEW_REACH + 24, X_IMM12_MAX);

        let far = VIEW_REACH + 8;
        let w = spilled_view_words(far);
        let mut add = Vec::new();
        slot::sp_add(&mut add, Gpr::X17, far);
        let add = words(&add);
        assert_eq!(&w[..add.len()], &add[..]);
        let at = add.len() + 1;
        assert_eq!(
            &w[at..at + 3],
            &[
                ldr_x_imm12(Gpr::X11, Gpr::X17, 16),
                ldr_x_imm12(Gpr::X12, Gpr::X17, 8),
                ldr_x_imm12(Gpr::X16, Gpr::X17, 24),
            ]
        );
    }

    /// The view living in x0 and the cell in x1 cross over on the slow
    /// arm without either being lost.
    #[test]
    fn crossed_argument_homes_are_staged_through_x17() {
        let a = alloc([(0, Gpr::X1), (1, Gpr::X0), (2, Gpr::X1)]);
        let inst = call(OP.func, vec![v(0), v(1)]);
        let (mut bytes, mut relocs) = (Vec::new(), Vec::new());
        assert!(try_emit(&mut bytes, &mut relocs, &inst, &a, &[], OP));
        let w = words(&bytes);
        let bl = relocs[0].byte_offset as usize / 4;
        assert_eq!(
            &w[bl - 3..=bl + 1],
            &[
                mov_x_reg(Gpr::X17, Gpr::X0),
                mov_x_reg(Gpr::X0, Gpr::X1),
                mov_x_reg(Gpr::X1, Gpr::X17),
                bl_imm26(0),
                mov_x_reg(Gpr::X1, Gpr::X0),
            ]
        );
    }
}
