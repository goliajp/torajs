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
}

impl<'a> St<'a> {
    /// The replacer-free state — what the historical entry points
    /// (`__torajs_anyv_json_stringify` and the `_gap` form the static
    /// unfold splices any-typed members through) build.
    pub(crate) fn plain(gap: &'a [u8]) -> Self {
        St {
            gap,
            replacer: None,
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
        let st = St {
            gap: bytes,
            // §25.5.2 step 4 — only an Object is consulted, and step
            // 4.a only a callable one. A string / number / null in
            // that slot is discarded by the spec, so `None` here is
            // the spec's own answer, not a shortcut.
            replacer: closure_boxed_entry(replacer),
        };
        stringify_state(v, &st, depth.max(0) as u32)
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
        let (t, val) = (
            crate::__torajs_anyv_unbox_tag(v),
            crate::__torajs_anyv_unbox_value(v),
        );
        crate::payload_rc_inc(t, val);
        __torajs_dynobj_set(&mut holder, key.cast(), t as u64, val as u64);
        let out = apply(st, box_void_ptr(holder), key.cast(), v, owned);
        __torajs_str_drop(key.cast());
        crate::nanbox_ffi::__torajs_anyv_rc_dec(box_void_ptr(holder));
        out
    }
}
