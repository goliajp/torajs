//! Class-method reification — the own `C.prototype.<m>` function
//! object (RFC 20260717-class-first-class-value knife B cut 1).
//!
//! tr dispatches class methods through the nominal vtable + the
//! `.__class_methods_<i>` boxed-adapter table; the prototype dynobj
//! itself carried no own method entries, so
//! `Object.getOwnPropertyDescriptor(C.prototype, "m")` answered
//! undefined and `C.prototype.m()` missed. This module mints the
//! introspection-side function object, mirroring
//! [`crate::method_value`]'s builtin-method cell shape:
//!
//! - Layout is a capture-less closure env (universal header +
//!   fn_addr + drop_fn + props + boxed_entry + one capture slot
//!   holding the ADAPTER vaddr), so every existing callable probe
//!   (`typeof` → "function", strict-eq identity, expando reads)
//!   works unchanged.
//! - `FLAG_STATIC_LITERAL` — rc traffic no-ops, the cycle collector
//!   skips it, the cell never drops (the prototype entry that owns
//!   it is itself a process-lifetime singleton).
//! - `boxed_entry` points at [`class_bare_entry`] — a bare call
//!   (direct, HOF callback, any-call) is the ES `this = undefined`
//!   TypeError. The sentinel doubles as the recognizer:
//!   [`class_method_adapter`] answers the carried adapter vaddr only
//!   for cells whose boxed_entry IS the sentinel (the same
//!   discrimination scheme `builtin_method_mid` uses).
//! - Method-call sites that resolve a dynobj entry to one of these
//!   cells invoke the adapter through the uniform boxed ABI with the
//!   RECEIVER in the env slot (`adapter(this-as-env, argv, argc)`),
//!   the exact shape `struct_method` dispatch uses.

use core::ffi::c_void;

use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::VALUE_UNDEFINED;

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

// Cell layout offsets — mirror of `method_value.rs` (itself a mirror
// of torajs-core's closure-env constants).
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_CAP_BASE_OFF: usize = 48;
const CELL_SIZE: usize = 56;

/// Boxed dual entry of every reified class-method cell — a bare call
/// is the ES `this = undefined` TypeError. Also the recognizer
/// sentinel for [`class_method_adapter`].
unsafe extern "C" fn class_bare_entry(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"class method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    VALUE_UNDEFINED
}

/// Native entry — an any→typed fn-slot cast direct-calls this
/// instead of jumping to 0; the pending throw propagates at the
/// callee boundary.
unsafe extern "C" fn class_native_entry() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"class method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    0
}

/// Mint one immortal class-method function object carrying `adapter`
/// (a `.__class_methods_<i>` boxed adapter vaddr). Called once per
/// (class, method) from `__torajs_anyv_class_register`'s wiring at
/// module init — the prototype's own entry keeps the sole reference,
/// so no interning table is needed for identity (`C.prototype.m ===
/// C.prototype.m` reads the same entry).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_method_cell_new(adapter: u64) -> *mut u8 {
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = class_native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = class_bare_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = adapter;
        cell
    }
}

/// The carried adapter vaddr when `ptr` is a reified class-method
/// cell (its boxed_entry is the [`class_bare_entry`] sentinel);
/// `None` for every other closure cell.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
pub(crate) unsafe fn class_method_adapter(ptr: *mut c_void) -> Option<u64> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == class_bare_entry as *const () as u64 {
            Some(*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64))
        } else {
            None
        }
    }
}
