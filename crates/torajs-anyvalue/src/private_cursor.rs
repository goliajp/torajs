//! RFC 20260916-private-element-brand slice 2a — the private-method
//! brand's per-instance install cursor (`torajs-core`
//! `ast/private_brand.rs` for the model).
//!
//! The cursor is the high half of a `Tag::Obj` cell's +8 slot, whose
//! low half is the class tag. Every compiler-side writer stores that
//! slot as a whole I64 with a zero high half at allocation, and every
//! reader of the tag masks, so the half is this module's alone after
//! the mint, except on an object literal's cell that left an optional
//! member out: there the half is that absent set while
//! `torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD` is up. Every reader here
//! settles such a set first ([`settle_absent`]), which zeroes the half.

use crate::member_get_layout::recv_cell;
use crate::nanbox::AnyValue;
use core::ffi::c_void;
use torajs_rc::Tag;

const CURSOR_OFF: usize = 12;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-rc — the header's FLAG_NON_EXTENSIBLE, cleared.
    fn __torajs_obj_is_extensible(p: *const c_void) -> bool;
    /// torajs-meta — the tag a unit class object records (-1 = not one).
    fn __torajs_class_unit_tag_of(cell: *const c_void) -> i64;
    /// torajs-meta — an absent-member set turned into deletion records.
    fn __torajs_obj_absent_settle(cell: *mut c_void);
}

/// Turn the absent-member set of the heap cell `p` into the deletion
/// records every runtime own-property face reads, when it carries one
/// (torajs-meta `struct_absent`): one header test otherwise.
///
/// # Safety
/// `p` is a live heap cell.
#[inline]
pub(crate) unsafe fn settle_absent(p: *const c_void) {
    let word = unsafe { p.cast::<u8>().add(4).cast::<u32>().read() };
    let absent = u32::from(torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD) << 16;
    if word & 0xFFFF == Tag::Obj as u32 && word & absent != 0 {
        unsafe { __torajs_obj_absent_settle(p.cast_mut()) };
    }
}

fn instance_cell(v: AnyValue) -> Option<*mut c_void> {
    match recv_cell(v) {
        Some((p, t)) if t == Tag::Obj as u16 => Some(p),
        _ => None,
    }
}

/// §7.3.33 PrivateMethodOrAccessorAdd / §7.3.32 PrivateFieldAdd for a
/// run of one class's elements: the object must be extensible
/// (proposal nonextensible-applies-to-private — a base ctor's
/// `Object.preventExtensions(this)` refuses the subclass's elements),
/// the cursor must stand at `from` (anywhere else, the brand is already
/// there) and moves to `to`. A value that is not a class instance cell
/// has no cursor; the covered chains never install on one.
///
/// # Safety
/// `v` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_priv_install(v: AnyValue, from: i64, to: i64) {
    let Some(p) = instance_cell(v) else { return };
    unsafe {
        settle_absent(p);
        if !__torajs_obj_is_extensible(p) {
            __torajs_throw_type_error(
                c"cannot add a private element to a non-extensible object".as_ptr(),
            );
            return;
        }
        let slot = p.cast::<u8>().add(CURSOR_OFF).cast::<u32>();
        if i64::from(slot.read()) != from {
            __torajs_throw_type_error(
                c"cannot add a private element twice to the same object".as_ptr(),
            );
            return;
        }
        slot.write(to as u32);
    }
}

/// The install cursor of a class instance cell; -1 for any other value.
///
/// # Safety
/// `v` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_priv_cursor(v: AnyValue) -> i64 {
    match instance_cell(v) {
        Some(p) => unsafe {
            settle_absent(p);
            i64::from(p.cast::<u8>().add(CURSOR_OFF).cast::<u32>().read())
        },
        None => -1,
    }
}

/// The word at byte `off` of a class instance cell (a unit class's
/// hidden evaluation slot, RFC 20260917-class-eval-unit K2). A unit
/// class object answers itself: a static body runs for the evaluation
/// its `this` IS. 0 for any other value. The caller has already established that `v` is an
/// instance of the class whose layout puts the slot at `off`.
///
/// # Safety
/// `v` is a live AnyValue; for an instance cell, `off` is inside it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_priv_unit_word(v: AnyValue, off: i64) -> i64 {
    match recv_cell(v) {
        Some((p, t)) if t == Tag::Obj as u16 => unsafe {
            p.cast::<u8>().add(off as usize).cast::<i64>().read()
        },
        Some((p, _)) if unsafe { __torajs_class_unit_tag_of(p) } >= 0 => p as i64,
        _ => 0,
    }
}

/// §7.3.31 / §7.3.32 step 1 — the object does not have the element.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_priv_brand_fail() {
    unsafe {
        __torajs_throw_type_error(c"the object does not have this private element".as_ptr());
    }
}
