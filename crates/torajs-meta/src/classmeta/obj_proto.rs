//! The [[Prototype]] of a `Tag::Obj` struct instance — the one place
//! the runtime turns an instance into its class prototype.
//!
//! A struct carries no prototype slot of its own: its class tag (the
//! low half of the +8 header word; the high half is the private-element
//! install cursor) names the class, and the class registered its
//! prototype under that tag. Six runtime faces used to spell this read
//! themselves, with three different accessors — two of which skipped
//! the generic-specialization fallback, and one of which read the whole
//! 64-bit word and lost the answer on any instance whose class declares
//! a private field. Asking here keeps them in agreement, and gives the
//! per-evaluation class unit (RFC 20260917) one place to answer from.

use core::ffi::c_void;

use super::__torajs_rc_inc;

/// Class prototype of `cell` as a BORROWED AnyValue immediate, 0 when
/// the class registered none (an anonymous layout, a tag past the
/// table).
///
/// This symbol is a by-tag registry READER for the link-time class
/// register judgment (`cmd_build_elide_class::REGISTRY_READERS`): a
/// program that can reach it keeps its register calls. In-crate
/// callers go through the symbol too, and it is never inlined, so the
/// judgment sees every read.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn __torajs_obj_proto_borrowed(cell: *const c_void) -> u64 {
    // SAFETY: a live Tag::Obj cell has its class tag at +8 (u32).
    let class_tag = unsafe { cell.cast::<u8>().add(8).cast::<u32>().read() };
    super::proto_anyv_borrowed(i64::from(class_tag))
}

/// Take a share of a borrowed prototype AnyValue: `VALUE_NULL_IMM` for
/// none, the value itself (rc +1 when it is a cell) otherwise — the
/// owned answer `Object.getPrototypeOf` hands back.
pub(crate) fn share_proto(v: u64) -> u64 {
    if v == 0 {
        return super::VALUE_NULL_IMM;
    }
    if super::is_cell_imm(v) {
        // SAFETY: cell-encoded AnyValue → 48-bit user-VA pointer to a
        // live heap object.
        unsafe { __torajs_rc_inc(v as *mut c_void) };
    }
    v
}
