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

/// [[Prototype]] of struct `cell` as a BORROWED AnyValue immediate:
/// its user link when it was re-parented (`VALUE_NULL_IMM` for an
/// explicit null), else its class prototype, 0 when the class
/// registered none (an anonymous layout, a tag past the table).
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
    // SAFETY: a live cell carries its header flags at +6.
    if unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() } & FLAG_OBJ_USER_PROTO != 0 {
        return unsafe { user_proto(cell) };
    }
    // SAFETY: a live Tag::Obj cell has its class tag at +8 (u32).
    let class_tag = i64::from(unsafe { cell.cast::<u8>().add(8).cast::<u32>().read() });
    // A unit class's instance names its own evaluation's class object
    // (RFC 20260917-class-eval-unit K1); the prototype registered under
    // its tag is only the shared template.
    match super::unit::unit_slot_off(class_tag) {
        0 => super::proto_anyv_borrowed(class_tag),
        off => unsafe { super::unit::unit_instance_proto(cell, off) },
    }
}

/// `torajs_rc::FLAG_OBJ_USER_PROTO` mirror (flags bit 6, Tag::Obj).
pub(crate) const FLAG_OBJ_USER_PROTO: u16 = 1 << 6;

/// Raise `FLAG_OBJ_USER_PROTO` on a struct instance whose user link was
/// just written. The flag's one writer: its text liveness is the link
/// judgment's evidence that a struct can carry a user [[Prototype]]
/// (`__torajs_struct_lacks_in_user_proto_slow`), so it is never inlined.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell whose props dict holds the link.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn __torajs_struct_user_proto_raise(cell: *mut c_void) {
    let flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>() };
    unsafe { flags.write(flags.read() | FLAG_OBJ_USER_PROTO) };
}

/// Props dynobj slot of a `Tag::Obj` cell — mirror of torajs-core
/// `ssa_lower.rs::OBJ_PROPS_OFF`.
pub(crate) const OBJ_PROPS_OFF: usize = 24;

/// The user [[Prototype]] of a re-parented struct instance (RFC
/// 20260920-struct-user-proto), BORROWED: the `\x00proto` entry of its
/// props dict, or `VALUE_NULL_IMM` when the link is an explicit null.
/// Unlike the class-tag answer, null here is a real answer — the chain
/// ends; callers must not read it as "fall to %Object.prototype%".
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell with `FLAG_OBJ_USER_PROTO` raised.
unsafe fn user_proto(cell: *const c_void) -> u64 {
    unsafe {
        let props = cell
            .cast::<u8>()
            .add(OBJ_PROPS_OFF)
            .cast::<*const c_void>()
            .read();
        let key = crate::reflect::alloc_str_key(crate::reflect::PROTO_SLOT_KEY);
        // The bit goes up only after the entry or the null bit is
        // written, so an absent entry never reaches here.
        let v = crate::reflect_proto_set::current_proto(props, key);
        super::__torajs_str_drop(key);
        v
    }
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
