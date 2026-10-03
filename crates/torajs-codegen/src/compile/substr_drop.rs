//! `__torajs_substr_drop(v)` opened at the call site.
//!
//! Most substring releases do nothing or decrement one count: a
//! borrowed view is marked static, an inline view on the stack only
//! releases its parent, and a parent or heap view still held elsewhere
//! just loses a count. Strings have no outgoing references, so a count
//! that stays above zero never needs the cycle buffer. Only the last
//! reference — which frees, or returns a view to its pool — makes the
//! call.

use core::mem::offset_of;

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};
use torajs_rc::HeapHeader;
use torajs_str::substr::{FLAG_SUBSTR_INLINE, SUBSTR_PARENT_OFF};

use super::rc::STATIC_BIT;
use super::rc_dec::{Fix, patch, place, store};
use super::{OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_operand_gpr, write_u32};
use crate::enc::{bl_imm26, cmp_w_imm, ldr_x_imm12, mov_x_reg, str_w_imm12, sub_w_imm};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

const SUBSTR_DROP_NAME: &str = "__torajs_substr_drop";
const INLINE_BIT: u32 =
    offset_of!(HeapHeader, flags) as u32 * 8 + FLAG_SUBSTR_INLINE.trailing_zeros();
const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);

/// The module's substring release.
#[derive(Debug, Clone, Copy)]
pub struct SubstrDrop {
    func: FuncId,
}

impl SubstrDrop {
    /// `None` leaves every release a plain call: a module that never
    /// declares the kernel, and `TORAJS_SUBSTR_DROP_INLINE_OFF=1`
    /// (bisect gate).
    pub fn of_module(module: &Module) -> Option<SubstrDrop> {
        if std::env::var_os("TORAJS_SUBSTR_DROP_INLINE_OFF").is_some_and(|v| v == "1") {
            return None;
        }
        let at = module
            .funcs
            .iter()
            .position(|f| f.name == SUBSTR_DROP_NAME)?;
        Some(SubstrDrop {
            func: FuncId(at as u32),
        })
    }

    /// Whether `kind` is a release of a value [`try_emit`] opens.
    pub(crate) fn matches(self, kind: &InstKind) -> bool {
        matches!(kind, InstKind::Call(fid, args)
            if *fid == self.func && matches!(args.as_slice(), [Operand::Value(_)]))
    }
}

/// The release at `inst`, opened inline; `false` when it is not one.
///
/// ```text
///     cbz   xV, done
///     ldr   x10, [xV]
///     tbnz  x10, #STATIC_BIT, done        ; a borrowed view
///     tbnz  x10, #INLINE_BIT, parent      ; a view on the stack
///     cmp   w10, #1 ; b.ls slow           ; the last reference
///     sub   w10, w10, #1 ; str w10, [xV] ; b done
/// parent:
///     ldr   x11, [xV, #parent] ; cbz x11, done
///     ldr   x10, [x11]
///     tbnz  x10, #STATIC_BIT, done        ; a literal is never counted
///     cmp   w10, #1 ; b.ls slow
///     sub   w10, w10, #1 ; str w10, [x11] ; b done
/// slow:
///     <store the call saves> ; mov x0, xV ; bl substr_drop ; <reload>
/// done:
/// ```
pub(super) fn try_emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
    op: SubstrDrop,
) -> bool {
    if !op.matches(&inst.kind) {
        return false;
    }
    let InstKind::Call(fid, args) = &inst.kind else {
        return false;
    };
    let v = materialize_operand_gpr(bytes, &args[0], OP_SCRATCH_LHS, alloc);
    let (hdr, parent) = (OP_SCRATCH_RHS, OP_SCRATCH_TMP);
    let mut done = Vec::new();
    let mut slow = Vec::new();

    done.push(place(bytes, Fix::Cbz(v)));
    write_u32(bytes, ldr_x_imm12(hdr, v, 0));
    done.push(place(bytes, Fix::Tbnz(hdr, STATIC_BIT)));
    let to_parent = place(bytes, Fix::Tbnz(hdr, INLINE_BIT));
    release_one(bytes, hdr, v, &mut slow, &mut done);

    let parent_at = bytes.len();
    patch(bytes, to_parent, parent_at);
    write_u32(bytes, ldr_x_imm12(parent, v, SUBSTR_PARENT_OFF as u32));
    done.push(place(bytes, Fix::Cbz(parent)));
    write_u32(bytes, ldr_x_imm12(hdr, parent, 0));
    done.push(place(bytes, Fix::Tbnz(hdr, STATIC_BIT)));
    release_one(bytes, hdr, parent, &mut slow, &mut done);

    let slow_at = bytes.len();
    for &(reg, off) in saves {
        store(bytes, reg, off, true);
    }
    if v != Gpr::X0 {
        write_u32(bytes, mov_x_reg(Gpr::X0, v));
    }
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::CallSite {
            target: CallTarget::Func(*fid),
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
    true
}

/// Take one count off the cell at `cell` whose header word is in
/// `hdr`, or go to the slow arm when it is the last one.
fn release_one(
    bytes: &mut Vec<u8>,
    hdr: Gpr,
    cell: Gpr,
    slow: &mut Vec<(usize, Fix)>,
    done: &mut Vec<(usize, Fix)>,
) {
    write_u32(bytes, cmp_w_imm(hdr, 1));
    slow.push(place(bytes, Fix::Cond(crate::enc::cond::LS)));
    write_u32(bytes, sub_w_imm(hdr, hdr, 1));
    write_u32(bytes, str_w_imm12(hdr, cell, 0));
    done.push(place(bytes, Fix::B));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_bits() {
        assert_eq!(INLINE_BIT, 48);
        assert_eq!(STATIC_BIT, 50);
        assert_eq!(SUBSTR_PARENT_OFF, 16);
    }
}
