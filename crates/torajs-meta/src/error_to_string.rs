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
    // torajs-str — undefined sentinel identity probe (RFC 20260710
    // C1); the own-absence read of the `message` slot (刀 2).
    fn __torajs_str_is_undef(p: *const u8) -> i64;
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
    // RFC 20260718-error-message-own-prop 刀 2 — an own-ABSENT
    // message (the undefined sentinel in the slot; no ctor message /
    // deleted) reads as `""` here per §20.5.3.4 step 8 (msg is ""
    // when the Get is undefined).
    let msg_absent = msg_ptr.is_null() || unsafe { __torajs_str_is_undef(msg_ptr) } != 0;
    // §20.5.3.4 step: an empty name yields the bare message.
    if name_len == 0 {
        if msg_absent {
            return unsafe { __torajs_str_alloc(b"".as_ptr(), 0) };
        }
        unsafe { __torajs_rc_inc(msg_ptr as *mut c_void) };
        return msg_ptr as *mut u8;
    }
    // An empty message yields the bare name (name is non-empty here).
    if msg_len == 0 || msg_absent {
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

/// `Error.isError(x)` (ES2025 §20.5.2.1) — the [[ErrorData]] probe.
/// tr's [[ErrorData]] IS the `FLAG_ERROR` header bit (bit 7) every
/// injected-error-class factory stamps on its `Tag::Obj` instances,
/// so the answer is a cell check + one flag read (RFC
/// 20260718-builtin-error-ctor-first-class 刀 3). `v` is borrowed.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; cell case points to a
/// live heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_is_error(v: u64) -> bool {
    // NaN-box cell test (mirror classmeta::is_cell_imm — top16 clear,
    // type-other bit clear, nonzero).
    if (v & 0xFFFF_0000_0000_0000) != 0 || (v & 0x02) != 0 || v == 0 {
        return false;
    }
    let p = v as *const u8;
    // Universal header: type_tag u16 @+4 (Tag::Obj = 1), flags u16
    // @+6 (FLAG_ERROR = 1 << 7).
    let tag = unsafe { (p.add(4) as *const u16).read() };
    if tag != 1 {
        return false;
    }
    let flags = unsafe { (p.add(6) as *const u16).read() };
    flags & (1 << 7) != 0
}
