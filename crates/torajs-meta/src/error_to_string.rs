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
// universal 32-byte header (blade 1: props dynobj @ +24), then
// Str-pointer fields `message` @+32 and `name` @+40. Str length is a
// u32 at +8 (`torajs-str` STR_LEN_OFF). The `name` offset is no
// longer read here: that slot normally holds the own-absence
// sentinel, so the value comes from the resolver instead.
const OBJ_MESSAGE_OFF: usize = 32;
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
    // §20.5.3.2 — `name` normally lives on `<C>.prototype`, not on the
    // instance, so the slot at the fixed offset holds the own-absence
    // sentinel unless user code assigned `this.name`. The resolver
    // answers the own value when there is one and the prototype
    // chain's otherwise; reading the slot raw here printed the
    // sentinel's own text ("undefined") as the error's name.
    let name_ptr = unsafe { __torajs_error_name_get(p.cast()) } as *const u8;
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

// Generic-lane FFI (obj_assign's per-key [[Get]] posture): the
// member-get tag/value channels answer borrow-shaped bits, the
// accessor entry runs its getter through `any_accessor_get` (owned),
// and `anyv_to_str` is the full §7.1.17 ToString (heap receivers run
// OrdinaryToPrimitive; symbol / throwing toString record a pending
// throw the checks below abort on).
unsafe extern "C" {
    /// torajs-anyvalue — `name` resolved through own-slot then the
    /// class prototype chain (the own-absence sentinel is the
    /// ordinary state; §20.5.3.2). Borrowed Str, never NULL: a fully
    /// missing chain answers the undefined sentinel.
    fn __torajs_error_name_get(obj: *const c_void) -> *mut u8;
    fn __torajs_any_member_get_tag(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_member_get_value(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_accessor_get(recv: u64, key: *const c_void, pair_bits: u64) -> u64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `struct_probe::ANY_ACCESSOR_TAG` mirror (obj_assign's twin).
const ACCESSOR_TAG: u64 = 6;
/// AnySlotTag undefined lane (absent probe answer).
const ANY_UNDEF_TAG: u64 = 5;

/// NaN-box heap-cell test (classmeta `is_cell_imm` mirror). Raw —
/// never unboxes, so a ShortStr immediate is NOT treated as a cell
/// (`unbox_value` would materialize an owned Str nobody releases).
#[inline]
fn anyv_cell(v: u64) -> Option<*mut c_void> {
    if (v & 0xFFFF_0000_0000_0000) != 0 || (v & 0x02) != 0 || v == 0 {
        return None;
    }
    Some(v as *mut c_void)
}

/// `? ToString(? Get(O, key))` for one §20.5.3.4 step — `Ok(None)`
/// when the Get answers undefined (the caller applies the spec
/// default), `Err(())` when the Get / ToString left a pending throw
/// (the caller aborts), `Ok(Some(str))` an owned Str otherwise.
unsafe fn get_tostring_step(recv: u64, lit: &[u8]) -> Result<Option<*mut u8>, ()> {
    unsafe {
        let key = __torajs_str_alloc(lit.as_ptr(), lit.len() as i64) as *mut c_void;
        let tag = __torajs_any_member_get_tag(recv, key);
        if __torajs_throw_check() != 0 {
            __torajs_str_drop(key as *mut u8);
            return Err(());
        }
        let (owned_av, is_owned) = if tag == ACCESSOR_TAG {
            let pair_bits = __torajs_any_member_get_value(recv, key);
            let got = __torajs_any_accessor_get(recv, key, pair_bits);
            if __torajs_throw_check() != 0 {
                if let Some(cell) = anyv_cell(got) {
                    __torajs_value_drop_heap(cell);
                }
                __torajs_str_drop(key as *mut u8);
                return Err(());
            }
            (got, true)
        } else if tag == ANY_UNDEF_TAG {
            __torajs_str_drop(key as *mut u8);
            return Ok(None);
        } else {
            let v = __torajs_any_member_get_value(recv, key);
            (__torajs_anyv_box_from_pair(tag as i64, v as i64), false)
        };
        __torajs_str_drop(key as *mut u8);
        // An accessor getter may still answer undefined — same
        // spec default as the absent probe.
        if owned_av == crate::reflect::VALUE_UNDEFINED_IMM {
            return Ok(None);
        }
        let s = __torajs_anyv_to_str(owned_av);
        if is_owned && let Some(cell) = anyv_cell(owned_av) {
            __torajs_value_drop_heap(cell);
        }
        if __torajs_throw_check() != 0 {
            if !s.is_null() {
                __torajs_str_drop(s as *mut u8);
            }
            return Err(());
        }
        Ok(Some(s as *mut u8))
    }
}

/// `Error.prototype.toString` (§20.5.3.4) over ANY receiver — the
/// dedicated `ANY_METHOD_ERROR_TO_STRING` cell's dispatch body. A
/// `FLAG_ERROR` instance rides the fixed-offset fast lane above;
/// every other object runs the generic steps: `? Get(O, "name")`
/// (undefined → "Error") / `? Get(O, "message")` (undefined → "")
/// with each Get's / ToString's abrupt completion aborting. A
/// non-object receiver throws TypeError (step 2). Returns an owned
/// Str pointer, or NULL when a pending throw was recorded.
///
/// # Safety
/// `recv` carries a valid AnyValue bit pattern; cell case points to
/// a live heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_proto_to_string(recv: u64) -> *mut u8 {
    // Non-cell (primitive / null / undefined) → §20.5.3.4 step 2.
    if (recv & 0xFFFF_0000_0000_0000) != 0 || (recv & 0x02) != 0 || recv == 0 {
        unsafe {
            __torajs_throw_type_error(
                c"Error.prototype.toString requires that |this| be an Object".as_ptr(),
            );
        }
        return core::ptr::null_mut();
    }
    let p = recv as *const u8;
    let tag = unsafe { (p.add(4) as *const u16).read() };
    // Primitive-shaped cells (heap Str 0 / Symbol 7 / BigInt 10) are
    // not Objects either — same step 2 TypeError (a ShortStr is an
    // immediate and already rejected above).
    if matches!(tag, 0 | 7 | 10) {
        unsafe {
            __torajs_throw_type_error(
                c"Error.prototype.toString requires that |this| be an Object".as_ptr(),
            );
        }
        return core::ptr::null_mut();
    }
    let flags = unsafe { (p.add(6) as *const u16).read() };
    // FLAG_ERROR Tag::Obj instance — the fixed-offset fast lane.
    if tag == 1 && flags & (1 << 7) != 0 {
        return unsafe { __torajs_error_to_string(p) };
    }
    unsafe {
        let name = match get_tostring_step(recv, b"name") {
            Err(()) => return core::ptr::null_mut(),
            Ok(None) => __torajs_str_alloc(b"Error".as_ptr(), 5),
            Ok(Some(s)) => s,
        };
        let msg = match get_tostring_step(recv, b"message") {
            Err(()) => {
                __torajs_str_drop(name);
                return core::ptr::null_mut();
            }
            Ok(None) => __torajs_str_alloc(b"".as_ptr(), 0),
            Ok(Some(s)) => s,
        };
        // §20.5.3.4 steps 9-10 — empty-side special cases, else
        // `name + ": " + message`.
        if str_len(name) == 0 {
            __torajs_str_drop(name);
            return msg;
        }
        if str_len(msg) == 0 {
            __torajs_str_drop(msg);
            return name;
        }
        let colon = __torajs_str_alloc(b": ".as_ptr(), 2);
        let name_colon = __torajs_str_concat(name, colon);
        __torajs_str_drop(colon);
        __torajs_str_drop(name);
        let result = __torajs_str_concat(name_colon, msg);
        __torajs_str_drop(name_colon);
        __torajs_str_drop(msg);
        result
    }
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
