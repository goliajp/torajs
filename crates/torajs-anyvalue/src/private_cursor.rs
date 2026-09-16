//! RFC 20260916-private-element-brand slice 2a — the private-method
//! brand's per-instance install cursor (`torajs-core`
//! `ast/private_brand.rs` for the model).
//!
//! The cursor is the high half of a `Tag::Obj` cell's +8 slot, whose
//! low half is the class tag. Every compiler-side writer stores that
//! slot as a whole I64 with a zero high half at allocation, and every
//! reader of the tag masks, so the half is this module's alone after
//! the mint.

use crate::member_get_layout::recv_cell;
use crate::nanbox::AnyValue;
use core::ffi::c_void;
use torajs_rc::Tag;

const CURSOR_OFF: usize = 12;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

fn instance_cell(v: AnyValue) -> Option<*mut c_void> {
    match recv_cell(v) {
        Some((p, t)) if t == Tag::Obj as u16 => Some(p),
        _ => None,
    }
}

/// §7.3.33 PrivateMethodOrAccessorAdd / §7.3.32 PrivateFieldAdd for a
/// run of one class's elements: the cursor
/// must stand at `from` (anywhere else, the brand is already there) and
/// moves to `to`. A value that is not a class instance cell has no
/// cursor; the covered chains never install on one.
///
/// # Safety
/// `v` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_priv_install(v: AnyValue, from: i64, to: i64) {
    let Some(p) = instance_cell(v) else { return };
    unsafe {
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
        Some(p) => i64::from(unsafe { p.cast::<u8>().add(CURSOR_OFF).cast::<u32>().read() }),
        None => -1,
    }
}

/// §7.3.31 / §7.3.32 step 1 — the object does not have the element.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_priv_brand_fail() {
    unsafe {
        __torajs_throw_type_error(c"the object does not have this private element".as_ptr());
    }
}
