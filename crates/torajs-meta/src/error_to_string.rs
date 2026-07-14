//! `__torajs_error_to_string` — `Error.prototype.toString` (ES §20.5.3.4)
//! as a runtime helper over an Error class instance.
//!
//! tr models `Error` (and its NativeError subclasses) as a synthetic
//! class whose instances are `Tag::Obj` structs carrying `FLAG_ERROR`;
//! every subclass shares the same layout prefix, so `message` and
//! `name` live at fixed offsets (field0 @+24, field1 @+32, each a Str
//! pointer) — the same layout the uncaught reporter reads
//! (`torajs-throw`). Doing toString as a helper (rather than an
//! injected class method) keeps `toString` out of `method_owners`, so
//! the checker's method resolution for a plain `x.toString()` on a
//! primitive / any / unrelated class is unaffected.
//!
//! §20.5.3.4: `name === "" ? message : message === "" ? name
//!             : name + ": " + message`.

use core::ffi::c_void;

// OBJ instance layout — mirror of `torajs-throw`'s reader: the
// universal 24-byte header, then Str-pointer fields `message` @+24 and
// `name` @+32. Str length is a u32 at +8 (`torajs-str` STR_LEN_OFF).
const OBJ_MESSAGE_OFF: usize = 24;
const OBJ_NAME_OFF: usize = 32;
const STR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_str_concat(a: *const u8, b: *const u8) -> *mut u8;
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_rc_inc(p: *mut c_void);
}

/// Str code-unit count, or 0 for a NULL pointer (an empty / absent
/// Str slot). Read-only.
///
/// # Safety
/// `s` is NULL or points to a valid Str heap block.
#[inline]
unsafe fn str_len(s: *const u8) -> u32 {
    if s.is_null() {
        return 0;
    }
    unsafe { (s.add(STR_LEN_OFF) as *const u32).read() }
}

/// `Error.prototype.toString` (§20.5.3.4) for a `FLAG_ERROR` OBJ
/// instance at `p`. Returns a Str the caller owns: a fresh `name + ": "
/// + message` in the common case, or an rc-inc'd view of the existing
/// `name` / `message` field when the other side is empty (§20.5.3.4's
/// empty-name / empty-message special cases). The `name` / `message`
/// operands are borrowed — `__torajs_str_concat` reads them read-only
/// and the instance keeps its own references.
///
/// # Safety
/// `p` points to a live `FLAG_ERROR` OBJ heap instance (its `message` /
/// `name` fields at the fixed offsets hold Str pointers or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_to_string(p: *const u8) -> *mut u8 {
    let name_ptr = unsafe { (p.add(OBJ_NAME_OFF) as *const *const u8).read() };
    let msg_ptr = unsafe { (p.add(OBJ_MESSAGE_OFF) as *const *const u8).read() };
    let name_len = unsafe { str_len(name_ptr) };
    let msg_len = unsafe { str_len(msg_ptr) };
    // §20.5.3.4 step: an empty name yields the bare message.
    if name_len == 0 {
        if msg_ptr.is_null() {
            return unsafe { __torajs_str_alloc(b"".as_ptr(), 0) };
        }
        unsafe { __torajs_rc_inc(msg_ptr as *mut c_void) };
        return msg_ptr as *mut u8;
    }
    // An empty message yields the bare name (name is non-empty here).
    if msg_len == 0 {
        unsafe { __torajs_rc_inc(name_ptr as *mut c_void) };
        return name_ptr as *mut u8;
    }
    // name + ": " + message — two borrows-in, fresh-out concats.
    let colon = unsafe { __torajs_str_alloc(b": ".as_ptr(), 2) };
    let name_colon = unsafe { __torajs_str_concat(name_ptr, colon) };
    unsafe { __torajs_str_drop(colon) };
    let result = unsafe { __torajs_str_concat(name_colon, msg_ptr) };
    unsafe { __torajs_str_drop(name_colon) };
    result
}
