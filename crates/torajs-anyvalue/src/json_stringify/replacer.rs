//! `JSON.stringify(value, replacer, space)` — the §25.5.2 step 4.a
//! `[[ReplacerFunction]]` leg, plus the serializer state record the
//! whole walk now threads.
//!
//! Until this module the second argument was LOWERED AND DROPPED,
//! which made a written replacer a silent wrong rather than a missing
//! feature: `JSON.stringify({a:1,b:2}, (k,v) => v*100)` answered the
//! unmodified object and every gate agreed with it (the output is
//! valid JSON, byte-identical to what an identity replacer produces).
//! Rotation 387 turned that into a loud refusal; this is the
//! implementation the refusal was holding the place for.
//!
//! §25.5.2.2 SerializeJSONProperty(state, key, holder) runs
//! `? Call(replacerFunction, holder, «key, value»)` AFTER the
//! `toJSON` hook of step 2 — measured against bun, a `Date` field
//! reaches the replacer as its ISO string, not as the Date. So the
//! hook site is also the replacer site, at every one of the walk's
//! property positions: the synthetic root wrapper, array elements,
//! dynobj entries, and both struct field lanes.
//!
//! The root is §25.5.2 step 11: a fresh `{ "": value }` holder is
//! minted and handed to the replacer as `this` — observable, and
//! bun prints it (`JSON.stringify({p:7}, function(k,v){ … this … })`
//! sees `{"":{"p":7}}` first), so it is a real object rather than a
//! notional one.
//!
//! Ownership: `apply` takes the value BORROWED-or-OWNED (the caller's
//! flag) and always answers OWNED when a replacer ran — the invoke
//! convention — releasing the previous reference itself.

use core::ffi::c_void;

use super::*;
use crate::method_call_closure_dispatch::{closure_boxed_entry, invoke_with_this};
use crate::nanbox::box_void_ptr;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    /// The generic `? Get(holder, P)` pair channel — the PropertyList
    /// walk reads keys the holder may not own, including inherited
    /// ones, which is what the spec's Get asks for.
    fn __torajs_any_member_get_tag(recv: AnyValue, key: *const c_void) -> u64;
    fn __torajs_any_member_get_value(recv: AnyValue, key: *const c_void) -> u64;
}

/// The ES §25.5.2.1 serializer state this runtime carries. `Indent`
/// and `Stack` are the walk's own `depth` parameter and recursion cap
/// (see the parent module doc); what has to travel by hand is the
/// gap and the replacer.
pub(crate) struct St<'a> {
    /// §25.5.2.1 step 8's indent unit — empty for the compact form.
    pub gap: &'a [u8],
    /// §25.5.2 step 4.a `[[ReplacerFunction]]` — the callable's
    /// (env cell, boxed entry) pair. `None` when the argument is
    /// absent or not callable, which is exactly when the spec
    /// ignores it.
    pub replacer: Option<(*mut c_void, u64)>,
    /// §25.5.2.1 step 4.b `[[PropertyList]]` — the owned Str keys an
    /// ARRAY replacer names, in list order. Mutually exclusive with
    /// [`Self::replacer`] (step 4 takes the callable branch first),
    /// and consulted only by the OBJECT walks: SerializeJSONArray
    /// never looks at it.
    pub property_list: Option<Vec<*mut c_void>>,
}

impl<'a> St<'a> {
    /// The replacer-free state — what the historical entry points
    /// (`__torajs_anyv_json_stringify` and the `_gap` form the static
    /// unfold splices any-typed members through) build.
    pub(crate) fn plain(gap: &'a [u8]) -> Self {
        St {
            gap,
            replacer: None,
            property_list: None,
        }
    }
}

impl Drop for St<'_> {
    fn drop(&mut self) {
        if let Some(list) = self.property_list.take() {
            for key in list {
                unsafe { __torajs_str_drop(key) };
            }
        }
    }
}

/// `JSON.stringify(value, replacer, space)` with all three arguments.
/// `gap` is the already-normalized indent Str (NULL for the compact
/// form) and `depth` the nesting level to start indenting from, both
/// as in [`super::gap::__torajs_anyv_json_stringify_gap`].
///
/// # Safety
/// `v` / `replacer` carry valid AnyValue bit patterns; `gap` is a
/// live Str block or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_json_stringify_full(
    v: AnyValue,
    replacer: AnyValue,
    gap: *const u8,
    depth: i64,
) -> *mut u8 {
    unsafe {
        let bytes = if gap.is_null() {
            &[][..]
        } else {
            let len = (gap.add(STR_LEN_OFF) as *const u32).read() as usize;
            core::slice::from_raw_parts(gap.add(STR_DATA_OFF), len)
        };
        // §25.5.2 step 4 — only an Object is consulted; step 4.a
        // takes a callable, step 4.b an Array. A string / number /
        // null in that slot is discarded by the spec, so answering
        // neither is the spec's own answer, not a shortcut.
        let fun = closure_boxed_entry(replacer);
        let st = St {
            gap: bytes,
            property_list: if fun.is_none() {
                property_list(replacer)
            } else {
                None
            },
            replacer: fun,
        };
        stringify_state(v, &st, depth.max(0) as u32)
    }
}

/// §25.5.2.1 step 4.b — the PropertyList an Array replacer names.
/// Only String and Number elements (and their wrapper objects)
/// contribute, each converted with ToString and appended once;
/// everything else is skipped. `None` for a non-Array.
unsafe fn property_list(replacer: AnyValue) -> Option<Vec<*mut c_void>> {
    unsafe {
        if !is_cell(replacer)
            || (as_void_ptr(replacer).cast::<u8>().add(4) as *const u16).read() != Tag::Arr as u16
        {
            return None;
        }
        let len = crate::index_any_iter_len::__torajs_any_iter_len(replacer);
        let mut out: Vec<*mut c_void> = Vec::new();
        for i in 0..len {
            let elem = crate::index_any::__torajs_any_index_get(replacer, i);
            let name = property_name(elem);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(elem);
            let Some(name) = name else { continue };
            // Step 4.b.iv.f — "PropertyList does not contain prop".
            if out.iter().any(|&k| same_key(k, name)) {
                __torajs_str_drop(name);
                continue;
            }
            out.push(name);
        }
        Some(out)
    }
}

/// One PropertyList element's contribution — an OWNED Str for a
/// String / Number (or a String / Number wrapper object), `None` for
/// every other shape, which step 4.b leaves out of the list entirely
/// (a `null` or a plain object names no property).
unsafe fn property_name(elem: AnyValue) -> Option<*mut c_void> {
    unsafe {
        let named = is_short_str(elem)
            || is_int32(elem)
            || is_double(elem)
            || (is_cell(elem) && {
                let t = (as_void_ptr(elem).cast::<u8>().add(4) as *const u16).read();
                t == Tag::Str as u16
                    || t == Tag::StringWrapper as u16
                    || t == Tag::NumberWrapper as u16
            });
        if !named {
            return None;
        }
        Some(crate::nanbox_ffi::__torajs_anyv_to_str(elem))
    }
}

/// Byte equality of two Str cells — the list's dedup test.
unsafe fn same_key(a: *mut c_void, b: *mut c_void) -> bool {
    unsafe { crate::prop_has::key_bytes(a).as_bytes() == crate::prop_has::key_bytes(b).as_bytes() }
}

/// §25.5.2.4 SerializeJSONObject step 5 under a PropertyList — the
/// keys come from the list instead of the holder's own enumerable
/// names, and each one is read with a full `? Get(holder, P)`. A key
/// the holder does not have reads `undefined` and is omitted, which
/// is how the spec expresses "the list may name absent properties".
pub(super) unsafe fn write_object_list(sb: *mut c_void, holder: AnyValue, depth: u32, st: &St) {
    unsafe {
        __torajs_jsb_push_byte(sb, b'{');
        let mut emitted = false;
        for &key in st.property_list.as_deref().unwrap_or(&[]) {
            let tag = __torajs_any_member_get_tag(holder, key);
            let raw = __torajs_any_member_get_value(holder, key);
            let mut value = crate::len_get::box_probe_pair(tag, raw, holder);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                break;
            }
            // §25.5.2.2 step 2 — the toJSON hook, exactly as the
            // own-name walks apply it. Step 3 is unreachable here:
            // step 4 makes a PropertyList and a ReplacerFunction
            // mutually exclusive.
            if let Some(r) = crate::json_stringify_tojson::apply_tojson(value) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                value = r;
                if __torajs_throw_check() != 0 {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                    break;
                }
            }
            if serializes_to_nothing(value) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                continue;
            }
            if emitted {
                __torajs_jsb_push_byte(sb, b',');
            }
            emitted = true;
            push_indent(sb, depth + 1, st);
            __torajs_jsb_push_str_quoted(sb, key as *const u8);
            __torajs_jsb_push_byte(sb, b':');
            if !st.gap.is_empty() {
                __torajs_jsb_push_byte(sb, b' ');
            }
            write_value(sb, value, depth + 1, st);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            if __torajs_throw_check() != 0 {
                break;
            }
        }
        if emitted {
            push_indent(sb, depth, st);
        }
        __torajs_jsb_push_byte(sb, b'}');
    }
}

/// §25.5.2.2 step 3 — `? Call(replacerFunction, holder, «key,
/// value»)`. `key` is a live Str cell (borrowed). The answer is OWNED
/// whenever a replacer ran; the previous reference is released here
/// when `owned` said the caller held one.
pub(super) unsafe fn apply(
    st: &St,
    holder: AnyValue,
    key: *mut c_void,
    value: AnyValue,
    owned: bool,
) -> (AnyValue, bool) {
    let Some((env, entry)) = st.replacer else {
        return (value, owned);
    };
    unsafe {
        let argv = [box_void_ptr(key), value];
        let r = invoke_with_this(env, entry, holder, argv.as_ptr(), 2);
        if owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
        }
        (r, true)
    }
}

/// [`apply`] for an array element — the spec's key is
/// `! ToString(𝔽(index))`, minted here and released after the call.
/// `elem` is owned in and out (the element walk always holds a
/// reference), so no ownership flag travels.
pub(super) unsafe fn apply_index(st: &St, holder: AnyValue, i: i64, elem: AnyValue) -> AnyValue {
    if st.replacer.is_none() {
        return elem;
    }
    unsafe {
        // An array index is an exact integer, so the Number-to-String
        // conversion IS `ToString(𝔽(index))` — no separate digit loop.
        let key = __torajs_f64_to_str(i as f64);
        let (out, _) = apply(st, holder, key, elem, true);
        __torajs_str_drop(key);
        out
    }
}

/// [`apply`] for a name known only as raw bytes (a struct layout's
/// field name), which has to become a Str cell for the call.
pub(super) unsafe fn apply_named(
    st: &St,
    holder: AnyValue,
    name: *const u8,
    name_len: usize,
    value: AnyValue,
    owned: bool,
) -> (AnyValue, bool) {
    if st.replacer.is_none() {
        return (value, owned);
    }
    unsafe {
        let key = __torajs_str_alloc(name, name_len as i64);
        let out = apply(st, holder, key.cast(), value, owned);
        __torajs_str_drop(key.cast());
        out
    }
}

/// §25.5.2 step 11 — the root property. A fresh `{ "": value }`
/// wrapper becomes the holder so the replacer's `this` is the object
/// the spec says it is; the entry takes its own reference, which the
/// wrapper's release drops again.
pub(super) unsafe fn apply_root(st: &St, v: AnyValue, owned: bool) -> (AnyValue, bool) {
    if st.replacer.is_none() {
        return (v, owned);
    }
    unsafe {
        let mut holder = __torajs_dynobj_alloc();
        let key = __torajs_str_alloc(b"".as_ptr(), 0);
        // AnyValue-shaped inc (546-02): the pair-shaped
        // payload_rc_inc double-staked a ShortStr root's materialized
        // Str; no-op inc + the set's single transfer balance it.
        crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
        let (t, val) = (
            crate::__torajs_anyv_unbox_tag(v),
            crate::__torajs_anyv_unbox_value(v),
        );
        __torajs_dynobj_set(&mut holder, key.cast(), t as u64, val as u64);
        let out = apply(st, box_void_ptr(holder), key.cast(), v, owned);
        __torajs_str_drop(key.cast());
        crate::nanbox_ffi::__torajs_anyv_rc_dec(box_void_ptr(holder));
        out
    }
}
