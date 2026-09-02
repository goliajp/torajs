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
//!   agrees). The hook itself lives in `json_stringify_tojson`, which
//!   every property site consults; the arm here is what a Date that
//!   never passes one falls back to.
//! - `space` and a FUNCTION `replacer` are both served, threaded
//!   through the walk as the §25.5.2.1 state record (`replacer::St`).
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

mod composites;
pub(crate) mod gap;
pub(crate) mod replacer;

use replacer::St;

use crate::member_set::STR_LEN_OFF;
use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, box_double, box_void_ptr, is_bool,
    is_cell, is_double, is_int32, is_null, is_short_str, is_undefined,
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
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_substr_to_owned(v: *const u8) -> *mut c_void;

    // torajs-dynobj — run an accessor entry's getter (§25.5.2.2's
    // ? Get(holder, key); receiver borrowed, result owned).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;

    // torajs-dynobj own-entry enumeration (the print walker's API).
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;

    // torajs-date §25.5.2 toJSON leg.
    fn __torajs_date_to_json(d: *const c_void) -> *mut u8;

    // torajs-structmeta — walk a Tag::Obj struct cell's fields via the
    // toolchain-emitted `__torajs_class_layouts` table. Same helpers
    // the `Object.keys/values/entries` any-lane arms use
    // (`torajs-meta::struct_enum`).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    /// torajs-meta — the declared-field enumerability filter (RFC
    /// 20260806-declared-field-redefine); a header-bit no-op unless
    /// the instance was actually redefined.
    pub(crate) fn __torajs_obj_field_is_nonenumerable(
        cell: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> i64;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StructFieldName;
    // Fill `out_anyv` with the field's value as a BORROWED NaN-box;
    // returns 1 on hit, 0 for missing layout / absent field.
    fn __torajs_struct_field_read_anyv(
        obj: *mut c_void,
        name: *const u8,
        name_len: u32,
        out_anyv: *mut u64,
    ) -> i64;

    /// torajs-str — the shared undefined-Str sentinel. A `undefined`
    /// RESULT (top-level undefined / callable argument) has to travel
    /// through the Str-typed call slot as something the consumers
    /// (print / typeof / strict-eq) read back as undefined; a raw
    /// NULL would print "null" and lose the §25.5.2 distinction.
    fn __torajs_str_undef() -> *mut u8;
}

/// `(ptr, len)` field-name slice returned by
/// `__torajs_struct_field_name` — mirrors
/// `torajs-structmeta::StrSlice`.
#[repr(C)]
struct StructFieldName {
    ptr: *const u8,
    len: usize,
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
    unsafe { stringify_with_gap(v, core::ptr::null()) }
}

/// Shared body of the two entries — `gap` is the ES §25.5.2.1
/// indent unit as a normalized Str cell (NULL / empty for the
/// compact form).
unsafe fn stringify_with_gap(v: AnyValue, gap: *const u8) -> *mut u8 {
    unsafe { stringify_with_gap_at(v, gap, 0) }
}

/// [`stringify_with_gap`] starting from an arbitrary nesting level —
/// what an any-typed member of a statically unfolded composite needs
/// so its indentation continues its parent's instead of restarting.
unsafe fn stringify_with_gap_at(v: AnyValue, gap: *const u8, depth: u32) -> *mut u8 {
    unsafe { stringify_state(v, &St::plain(gap), depth) }
}

/// The walk proper — §25.5.2 step 11's root property under the state
/// record every recursion level threads.
unsafe fn stringify_state(v: AnyValue, st: &St, depth: u32) -> *mut u8 {
    unsafe {
        // §25.5.2.3 step 2 — the top-level value consults the user
        // toJSON hook before walking (rotation 205), and step 3 then
        // offers the result to the replacer.
        let (v, owned) = match crate::json_stringify_tojson::apply_tojson(v) {
            Some(r) => (r, true),
            None => (v, false),
        };
        if __torajs_throw_check() != 0 {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
            }
            return __torajs_str_undef();
        }
        let (v, owned) = replacer::apply_root(st, v, owned);
        if __torajs_throw_check() != 0 {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
            }
            return __torajs_str_undef();
        }
        let sb = __torajs_jsb_new(64);
        let wrote = write_value(sb, v, depth, st);
        if owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
        }
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
unsafe fn write_value(sb: *mut c_void, v: AnyValue, depth: u32, st: &St) -> Wrote {
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
        write_cell(sb, as_void_ptr(v), depth, st)
    }
}

/// The heap-tag arms.
unsafe fn write_cell(sb: *mut c_void, ptr: *mut c_void, depth: u32, st: &St) -> Wrote {
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        match tag {
            t if t == Tag::Str as u16 => {
                // A Substr view (a split-product slot) keeps its
                // bytes behind parent+offset; the quoting path reads
                // the owned layout, so materialize first — the raw
                // cell printed its own view-struct fields as garbage
                // characters. Flag bits mirror torajs-str substr.rs
                // (FLAG_SUBSTR_INLINE = 1<<0, FLAG_SUBSTR_VIEW =
                // 1<<10); flags live at header +6 (index_any idiom).
                let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
                if flags & ((1 << 0) | (1 << 10)) != 0 {
                    let owned = __torajs_substr_to_owned(ptr as *const u8);
                    __torajs_jsb_push_str_quoted(sb, owned as *const u8);
                    __torajs_str_drop(owned);
                } else {
                    __torajs_jsb_push_str_quoted(sb, ptr as *const u8);
                }
                Wrote::Value
            }
            t if t == Tag::Arr as u16 => {
                write_array(sb, ptr, depth, st);
                Wrote::Value
            }
            t if t == Tag::DynObj as u16 => {
                // §25.5.2 SerializeJSONProperty step 5a — a rawJSON
                // carrier (`JSON.rawJSON`, header bit
                // FLAG_DYNOBJ_RAW_JSON) splices its validated text
                // verbatim instead of walking the object shape. The
                // single frozen entry's value is the text Str
                // (borrowed from the entry — no drop).
                let hflags = (ptr.cast::<u8>().add(6) as *const u16).read();
                if hflags & torajs_rc::FLAG_DYNOBJ_RAW_JSON != 0 {
                    let raw = __torajs_dynobj_iter_value(ptr, 0);
                    __torajs_jsb_push_str_raw(sb, as_void_ptr(raw) as *const u8);
                    return Wrote::Value;
                }
                composites::write_object(sb, ptr, depth, st);
                Wrote::Value
            }
            t if t == Tag::Obj as u16 => {
                composites::write_struct(sb, ptr, depth, st);
                Wrote::Value
            }
            t if t == Tag::Date as u16 => {
                // §25.5.2 step 3 — Date.prototype.toJSON answers the
                // ISO string, which then serializes as a string; an
                // invalid date's toJSON is JS null (§21.4.4.37
                // steps 2-3, kernel NULL) and serializes bare.
                let iso = __torajs_date_to_json(ptr);
                if iso.is_null() {
                    push_bytes(sb, b"null");
                } else {
                    __torajs_jsb_push_str_quoted(sb, iso as *const u8);
                    __torajs_str_drop(iso as *mut c_void);
                }
                Wrote::Value
            }
            // A callable serializes to nothing, like undefined.
            t if t == Tag::Closure as u16 => Wrote::Nothing,
            // §25.5.2.4 step 4 — a primitive wrapper serializes as
            // its wrapped primitive, not as an ordinary object: step
            // 4.a unwraps [[NumberData]] / [[StringData]] /
            // [[BooleanData]], and steps 8-9 then write it. Payload
            // offsets mirror torajs-wrapper (f64 / inner Str cell /
            // byte, all at +8), read directly like the `coerce`
            // wrapper arms do — these classes are not user-patchable,
            // so the method-dispatch round trip buys nothing. Without
            // these the cells fell to the catch-all below and every
            // wrapper answered `{}`.
            // §25.5.2.4 — a Symbol has no JSON representation and is
            // NOT an error: it serializes to nothing, exactly like
            // `undefined` and a callable. The caller's three-way split
            // then answers `undefined` at the top level, omits the key
            // inside an object, and writes `null` inside an array. It
            // reached the catch-all below and answered `{}`.
            t if t == Tag::Symbol as u16 => Wrote::Nothing,
            // §25.5.2.4 step 10 — a BigInt has no JSON representation
            // and is a TypeError, not an object walk. It reached the
            // catch-all below and answered `{}`.
            t if t == Tag::BigInt as u16 => {
                __torajs_throw_type_error(c"Do not know how to serialize a BigInt".as_ptr());
                Wrote::Nothing
            }
            t if t == Tag::NumberWrapper as u16 => {
                // Step 9: a non-finite [[NumberData]] is `null`, which
                // write_double already encodes.
                write_double(sb, ((ptr as *const u8).add(8) as *const f64).read());
                Wrote::Value
            }
            t if t == Tag::StringWrapper as u16 => {
                let inner = ((ptr as *const u8).add(8) as *const *const c_void).read();
                if inner.is_null() {
                    // A NULL inner cell is the empty [[StringData]].
                    push_bytes(sb, b"\"\"");
                } else {
                    __torajs_jsb_push_str_quoted(sb, inner as *const u8);
                }
                Wrote::Value
            }
            t if t == Tag::BooleanWrapper as u16 => {
                let b = *((ptr as *const u8).add(8));
                push_bytes(sb, if b != 0 { b"true" } else { b"false" });
                Wrote::Value
            }
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
unsafe fn write_array(sb: *mut c_void, ptr: *mut c_void, depth: u32, st: &St) {
    unsafe {
        __torajs_jsb_push_byte(sb, b'[');
        let boxed = crate::nanbox::box_void_ptr(ptr);
        let len = crate::index_any_iter_len::__torajs_any_iter_len(boxed);
        for i in 0..len {
            if i > 0 {
                __torajs_jsb_push_byte(sb, b',');
            }
            push_indent(sb, depth + 1, st);
            let mut elem = crate::index_any::__torajs_any_index_get(boxed, i);
            // §25.5.2.3 step 2 — element-level toJSON hook.
            if let Some(r) = crate::json_stringify_tojson::apply_tojson(elem) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(elem);
                elem = r;
                if __torajs_throw_check() != 0 {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(elem);
                    break;
                }
            }
            // §25.5.2.2 step 3 — an element's property key is its
            // index; an `undefined` answer becomes `null` below, the
            // array lane's own version of dropping the value.
            elem = replacer::apply_index(st, boxed, i, elem);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(elem);
                break;
            }
            if write_value(sb, elem, depth + 1, st) == Wrote::Nothing {
                push_bytes(sb, b"null");
            }
            crate::nanbox_ffi::__torajs_anyv_rc_dec(elem);
            if __torajs_throw_check() != 0 {
                break;
            }
        }
        if len > 0 {
            push_indent(sb, depth, st);
        }
        __torajs_jsb_push_byte(sb, b']');
    }
}

/// JSON-quote `len` bytes at `ptr` into `sb` by materializing a
/// pooled Str cell and going through the builder's normal quoted
/// path — the raw-name slice returned by
/// `__torajs_struct_field_name` isn't itself a Str cell.
unsafe fn quote_bytes(sb: *mut c_void, ptr: *const u8, len: usize) {
    unsafe {
        let cell = __torajs_str_alloc(ptr, len as i64);
        __torajs_jsb_push_str_quoted(sb, cell as *const u8);
        __torajs_str_drop(cell as *mut c_void);
    }
}

/// The `AccessorPair` cell an entry's stored value points at, `None`
/// for every data shape — the `write_object` walk's accessor-entry
/// probe (heap-tag twin of dynobj `get_tag`'s `ANY_ACCESSOR`
/// sentinel).
unsafe fn accessor_pair_of(v: AnyValue) -> Option<*const c_void> {
    unsafe {
        if !is_cell(v) {
            return None;
        }
        let ptr = as_void_ptr(v);
        if (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::AccessorPair as u16 {
            Some(ptr as *const c_void)
        } else {
            None
        }
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

/// ES §25.5.2.1 — under a non-empty `gap`, every element / property
/// sits on its own line indented by one `gap` per nesting level, and
/// the closing bracket returns to the parent's level. A no-op for
/// the compact form, which is the whole cost this adds to it.
unsafe fn push_indent(sb: *mut c_void, depth: u32, st: &St) {
    if !st.has_gap() {
        return;
    }
    unsafe {
        __torajs_jsb_push_byte(sb, b'\n');
        // The gap goes in as a Str cell so its encoding travels with
        // it — a UTF-16 gap pushed byte-wise left the builder's
        // Latin-1 assumption in place (rotation 560).
        for _ in 0..depth {
            __torajs_jsb_push_str_raw(sb, st.gap);
        }
    }
}

/// Append raw (already-encoded) bytes to the builder.
unsafe fn push_bytes(sb: *mut c_void, bytes: &[u8]) {
    for b in bytes {
        unsafe { __torajs_jsb_push_byte(sb, *b) };
    }
}
