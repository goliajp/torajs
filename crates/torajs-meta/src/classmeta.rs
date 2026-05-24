//! Class / prototype registries keyed by class runtime tag —
//! port of `runtime_str.c` L820-867.
//!
//! Two parallel fixed-size arrays sized at `MAX_CLASSES = 256`:
//!
//! - `protos_by_tag[c]` — borrowed `*mut AnyBox` of the
//!   `__proto_<C>` LetDecl. Read by `Object.getPrototypeOf(instance)`.
//! - `classes_by_tag[c]` — borrowed `*mut AnyBox` of the
//!   `__class_<C>` LetDecl. Read by `__torajs_class_get` (P4.5
//!   `new.target` plumbing).
//!
//! Lifetime-of-process — the `__proto_<C>` / `__class_<C>` Any-boxes
//! live in module-scope let bindings whose lifetime spans the whole
//! program; no rc bump on register (the let binding keeps them
//! alive). `proto_get` / `class_get` rc_inc on every read so the
//! caller receives an OWNED Any-box.
//!
//! Concurrency: JS execution is single-threaded so unsynchronized
//! `static mut` matches the pre-port C bit-for-bit. Cross-thread
//! access from native plugins would be UB — never the model.

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_any_box(tag: i64, value: i64) -> *mut c_void;
    fn __torajs_rc_inc(p: *mut c_void);
}

const MAX_CLASSES: usize = 256;
const ANY_NULL: i64 = 0;
const ANY_UNDEF: i64 = 5;

static mut PROTOS_BY_TAG: [*mut c_void; MAX_CLASSES] = [core::ptr::null_mut(); MAX_CLASSES];
static mut CLASSES_BY_TAG: [*mut c_void; MAX_CLASSES] = [core::ptr::null_mut(); MAX_CLASSES];

#[inline]
fn in_range(tag: i64) -> bool {
    (0..MAX_CLASSES as i64).contains(&tag)
}

/// Register the class's `__proto_<C>` Any-box at module init.
/// No rc bump — the box is owned by its let binding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_register(tag: i64, proto_anybox: *mut c_void) {
    if !in_range(tag) {
        return;
    }
    // SAFETY: single-threaded JS runtime, no aliased writes.
    unsafe {
        PROTOS_BY_TAG[tag as usize] = proto_anybox;
    }
}

/// Register the class's `__class_<C>` Any-box (for `new.target`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_register(tag: i64, class_anybox: *mut c_void) {
    if !in_range(tag) {
        return;
    }
    // SAFETY: same as proto_register.
    unsafe {
        CLASSES_BY_TAG[tag as usize] = class_anybox;
    }
}

/// `Object.getPrototypeOf(instance)` → owned Any-box. Returns
/// `ANY_NULL` box on out-of-range tag or unregistered class.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_get(tag: i64) -> *mut c_void {
    if !in_range(tag) {
        return unsafe { __torajs_any_box(ANY_NULL, 0) };
    }
    // SAFETY: single-threaded JS; reading a registered pointer.
    let p = unsafe { PROTOS_BY_TAG[tag as usize] };
    if p.is_null() {
        return unsafe { __torajs_any_box(ANY_NULL, 0) };
    }
    unsafe { __torajs_rc_inc(p) };
    p
}

/// `new.target` lookup (`__torajs_class_get`) — owned Any-box.
/// Returns `ANY_UNDEF` box on out-of-range / unregistered class
/// (spec §13.3.10 — `new.target` outside `new` is undefined).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_get(tag: i64) -> *mut c_void {
    if !in_range(tag) {
        return unsafe { __torajs_any_box(ANY_UNDEF, 0) };
    }
    // SAFETY: same as proto_get.
    let p = unsafe { CLASSES_BY_TAG[tag as usize] };
    if p.is_null() {
        return unsafe { __torajs_any_box(ANY_UNDEF, 0) };
    }
    unsafe { __torajs_rc_inc(p) };
    p
}
