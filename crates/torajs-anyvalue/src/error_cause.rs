//! §20.5.8.1 InstallErrorCause — the non-enumerable `cause` install.
//!
//! The spec installs `cause` with CreateNonEnumerableDataPropertyOrThrow,
//! i.e. `{W:1, E:0, C:1}` — the same attributes `message` carries. The
//! injected constructors used to spell it as an ordinary
//! `(this as any).cause = options.cause`, which lands an ENUMERABLE
//! entry in the struct's expando dict, so `Object.keys(new Error("m",
//! {cause: 1}))` listed it and `JSON.stringify` serialized it.
//!
//! A user's own `err.cause = x` after construction stays an ordinary
//! assignment and stays enumerable — bun reports exactly that
//! difference, and it falls out of leaving the assignment path alone.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::recv_cell;
use crate::member_get_layout::OBJ_PROPS_OFF;
use crate::nanbox::AnyValue;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// `{ [[Writable]]: true, [[Enumerable]]: FALSE, [[Configurable]]:
/// true }` — `classmeta::DEFINE_CTOR_FLAGS` twin (bit 1, enumerable,
/// is the one deliberately clear).
const DEFINE_NONENUM_FLAGS: u64 = (1 << 6) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 0) | (1 << 2);

/// Install `cause` on an error instance as a non-enumerable own
/// property. `value` arrives OWNED and is transferred into the entry;
/// a receiver that is not a struct cell drops it and does nothing
/// (the injected ctor only ever calls this with `this`).
///
/// # Safety
/// `extern "C"` ABI. `recv` / `value` are NaN-box AnyValue immediates.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_install_cause(recv: AnyValue, value: AnyValue) {
    let Some((ptr, cell_tag)) = recv_cell(recv) else {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(value) };
        return;
    };
    if cell_tag != Tag::Obj as u16 {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(value) };
        return;
    }
    unsafe {
        let props_slot = ptr.cast::<u8>().add(OBJ_PROPS_OFF) as *mut *mut c_void;
        if (*props_slot).is_null() {
            // r502 — first attach through the rc entry the instance's
            // drop seams are guarded on.
            torajs_rc::obj_entry::__torajs_obj_props_attach(
                ptr.cast::<u8>(),
                __torajs_dynobj_alloc(),
            );
        }
        let key = __torajs_str_alloc(b"cause".as_ptr(), 5);
        let tag = crate::nanbox_encode::__torajs_anyv_unbox_tag(value) as u64;
        let val = crate::nanbox_encode::__torajs_anyv_unbox_value_owned(value) as u64;
        __torajs_dynobj_define_plain(
            props_slot,
            key as *mut c_void,
            tag,
            val,
            DEFINE_NONENUM_FLAGS,
        );
        __torajs_str_drop(key as *mut c_void);
    }
}
