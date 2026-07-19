//! `JSON.stringify(v)` over an `any`-lane value (RFC
//! 20260719-ns-static-value-reify B3b) — the runtime twin of the
//! typed tier's compile-time-unfolded serializer
//! (`ssa_lower_json_stringify`), which can only emit a walk for a
//! statically known shape and rejected `Type::Any` outright.
//!
//! ES §25.5.2 SerializeJSONProperty as implemented here:
//!
//! - `undefined` and callables serialize to NOTHING — at the top
//!   level the whole call answers `undefined` (NULL out), inside an
//!   array the slot becomes `null`, inside an object the key is
//!   omitted entirely. That three-way split is why the recursive
//!   writer returns a [`Wrote`] verdict instead of a bool.
//! - Non-finite numbers serialize as `null` (§25.5.2 step 10).
//! - `Date` answers its `toJSON` — the ISO string, quoted (bun
//!   agrees; the general user-defined `toJSON` hook is a recorded
//!   RFC boundary, as is a `replacer` / `space` argument).
//! - Cycles are caught by a depth cap rather than a visited set:
//!   the spec throws a TypeError on a cyclic structure, and a
//!   runaway recursion would blow the native stack before any
//!   diagnosis. The cap answers the same catchable TypeError.
//!
//! Output goes through torajs-str's `JsonBuilder`, the SAME builder
//! the typed tier's emitted walk pushes into, so both tiers produce
//! byte-identical text for a shape they can both express.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined,
};

/// torajs-dynobj `layout::BUCKET_FLAG_ENUMERABLE` mirror — a
/// non-enumerable own entry is not serialized (§25.5.2 walks
/// EnumerableOwnPropertyNames).
const BUCKET_FLAG_ENUMERABLE: u64 = 1 << 1;

/// Recursion cap standing in for the spec's cyclic-structure check
/// (see module doc). Deep-but-acyclic JSON well below this still
/// serializes; a cycle hits it in bounded time.
const MAX_DEPTH: u32 = 512;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;

    // torajs-str JsonBuilder — the typed tier's own output path.
    fn __torajs_jsb_new(initial_cap: u32) -> *mut c_void;
    fn __torajs_jsb_push_byte(sb: *mut c_void, b: u8);
    fn __torajs_jsb_push_str_raw(sb: *mut c_void, str_ptr: *const u8);
    fn __torajs_jsb_push_str_quoted(sb: *mut c_void, str_ptr: *const u8);
    fn __torajs_jsb_push_i64(sb: *mut c_void, n: i64);
    fn __torajs_jsb_finalize(sb: *mut c_void) -> *mut u8;

    // torajs-str / torajs-num conversions.
    fn __torajs_f64_to_str(n: f64) -> *mut c_void;
    fn __torajs_str_drop(s: *mut c_void);

    // torajs-dynobj own-entry enumeration (the print walker's API).
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;

    // torajs-date §25.5.2 toJSON leg.
    fn __torajs_date_to_iso_string(d: *const c_void) -> *mut u8;

    /// torajs-str — the shared undefined-Str sentinel. A `undefined`
    /// RESULT (top-level undefined / callable argument) has to travel
    /// through the Str-typed call slot as something the consumers
    /// (print / typeof / strict-eq) read back as undefined; a raw
    /// NULL would print "null" and lose the §25.5.2 distinction.
    fn __torajs_str_undef() -> *mut u8;
}

/// Whether a value contributed text — the §25.5.2 three-way split
/// (see module doc).
#[derive(PartialEq)]
enum Wrote {
    /// Text was appended.
    Value,
    /// `undefined` / callable — the caller decides between omitting
    /// the key, emitting `null`, or answering undefined.
    Nothing,
}

/// `JSON.stringify(v)` for an any-lane argument. Returns a freshly
/// owned Str the caller drops, or the undefined-Str sentinel when
/// the result is `undefined` (a top-level `undefined` / callable
/// argument, per §25.5.2) or when a pending throw was recorded.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_json_stringify(v: AnyValue) -> *mut u8 {
    unsafe {
        let sb = __torajs_jsb_new(64);
        let wrote = write_value(sb, v, 0);
        let s = __torajs_jsb_finalize(sb);
        if wrote == Wrote::Nothing || __torajs_throw_check() != 0 {
            __torajs_str_drop(s as *mut c_void);
            return __torajs_str_undef();
        }
        s
    }
}

/// Append one value's serialization. See [`Wrote`] for the
/// undefined/callable contract.
unsafe fn write_value(sb: *mut c_void, v: AnyValue, depth: u32) -> Wrote {
    unsafe {
        if depth > MAX_DEPTH {
            __torajs_throw_type_error(c"cyclic or too deeply nested structure".as_ptr());
            return Wrote::Nothing;
        }
        if is_undefined(v) {
            return Wrote::Nothing;
        }
        if is_null(v) {
            push_bytes(sb, b"null");
            return Wrote::Value;
        }
        if is_bool(v) {
            push_bytes(sb, if as_bool(v) { b"true" } else { b"false" });
            return Wrote::Value;
        }
        if is_int32(v) {
            __torajs_jsb_push_i64(sb, i64::from(as_int32(v)));
            return Wrote::Value;
        }
        if is_double(v) {
            write_double(sb, as_double(v));
            return Wrote::Value;
        }
        if is_short_str(v) {
            // Materialize the inline payload so the builder's
            // quoting path sees an ordinary Str block.
            let cell = crate::nanbox_ffi::__torajs_anyv_to_str(v);
            __torajs_jsb_push_str_quoted(sb, cell as *const u8);
            __torajs_str_drop(cell);
            return Wrote::Value;
        }
        if !is_cell(v) {
            return Wrote::Nothing;
        }
        write_cell(sb, as_void_ptr(v), depth)
    }
}

/// The heap-tag arms.
unsafe fn write_cell(sb: *mut c_void, ptr: *mut c_void, depth: u32) -> Wrote {
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        match tag {
            t if t == Tag::Str as u16 => {
                __torajs_jsb_push_str_quoted(sb, ptr as *const u8);
                Wrote::Value
            }
            t if t == Tag::Arr as u16 => {
                write_array(sb, ptr, depth);
                Wrote::Value
            }
            t if t == Tag::DynObj as u16 => {
                write_object(sb, ptr, depth);
                Wrote::Value
            }
            t if t == Tag::Date as u16 => {
                // §25.5.2 step 3 — Date.prototype.toJSON answers the
                // ISO string, which then serializes as a string.
                let iso = __torajs_date_to_iso_string(ptr);
                __torajs_jsb_push_str_quoted(sb, iso as *const u8);
                __torajs_str_drop(iso as *mut c_void);
                Wrote::Value
            }
            // A callable serializes to nothing, like undefined.
            t if t == Tag::Closure as u16 => Wrote::Nothing,
            // Map / Set / RegExp / Promise / … have no own
            // enumerable properties, so the spec's ordinary-object
            // walk answers `{}`.
            _ => {
                push_bytes(sb, b"{}");
                Wrote::Value
            }
        }
    }
}

/// `[...]` — an element that serializes to nothing becomes `null`
/// (§25.5.2 SerializeJSONArray step 8).
unsafe fn write_array(sb: *mut c_void, ptr: *mut c_void, depth: u32) {
    unsafe {
        __torajs_jsb_push_byte(sb, b'[');
        let boxed = crate::nanbox::box_void_ptr(ptr);
        let len = crate::index_any::__torajs_any_iter_len(boxed);
        for i in 0..len {
            if i > 0 {
                __torajs_jsb_push_byte(sb, b',');
            }
            let elem = crate::index_any::__torajs_any_index_get(boxed, i);
            if write_value(sb, elem, depth + 1) == Wrote::Nothing {
                push_bytes(sb, b"null");
            }
            crate::nanbox_ffi::__torajs_anyv_rc_dec(elem);
            if __torajs_throw_check() != 0 {
                break;
            }
        }
        __torajs_jsb_push_byte(sb, b']');
    }
}

/// `{...}` — own enumerable entries in §10.1.11.1 order (the print
/// walker's `iter_order` contract); a key whose value serializes to
/// nothing is omitted entirely.
unsafe fn write_object(sb: *mut c_void, ptr: *mut c_void, depth: u32) {
    unsafe {
        __torajs_jsb_push_byte(sb, b'{');
        let len = __torajs_dynobj_iter_len(ptr);
        let order_layout = core::alloc::Layout::from_size_align(len as usize * 8, 8).unwrap();
        let order = if len > 0 {
            std::alloc::alloc_zeroed(order_layout) as *mut u64
        } else {
            core::ptr::null_mut()
        };
        let n = __torajs_dynobj_iter_order(ptr, order, len);
        let mut emitted = false;
        for j in 0..n {
            let i = *order.add(j as usize);
            if __torajs_dynobj_iter_flags(ptr, i) & BUCKET_FLAG_ENUMERABLE == 0 {
                continue;
            }
            let value = __torajs_dynobj_iter_value(ptr, i);
            // Probe the value FIRST: an undefined / callable field
            // drops its key, so the separator and key bytes must not
            // be emitted speculatively. Serializing into a scratch
            // builder would cost an alloc per field — instead take
            // the cheap pre-check the split allows.
            if serializes_to_nothing(value) {
                continue;
            }
            if emitted {
                __torajs_jsb_push_byte(sb, b',');
            }
            emitted = true;
            __torajs_jsb_push_str_quoted(sb, __torajs_dynobj_iter_key(ptr, i) as *const u8);
            __torajs_jsb_push_byte(sb, b':');
            write_value(sb, value, depth + 1);
            if __torajs_throw_check() != 0 {
                break;
            }
        }
        if !order.is_null() {
            std::alloc::dealloc(order as *mut u8, order_layout);
        }
        __torajs_jsb_push_byte(sb, b'}');
    }
}

/// The object-field pre-check mirroring [`write_value`]'s
/// `Wrote::Nothing` arms — undefined and callables (the only two
/// shapes that drop their key).
unsafe fn serializes_to_nothing(v: AnyValue) -> bool {
    unsafe {
        if is_undefined(v) {
            return true;
        }
        if !is_cell(v) {
            return false;
        }
        let ptr = as_void_ptr(v);
        (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Closure as u16
    }
}

/// §25.5.2 step 10 — a non-finite number serializes as `null`.
unsafe fn write_double(sb: *mut c_void, x: f64) {
    unsafe {
        if !x.is_finite() {
            push_bytes(sb, b"null");
            return;
        }
        let s = __torajs_f64_to_str(x);
        __torajs_jsb_push_str_raw(sb, s as *const u8);
        __torajs_str_drop(s);
    }
}

unsafe fn push_bytes(sb: *mut c_void, bytes: &[u8]) {
    for b in bytes {
        unsafe { __torajs_jsb_push_byte(sb, *b) };
    }
}
