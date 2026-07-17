//! `__torajs_any_method_call` — `recv.name(args…)` where the
//! receiver is an `any` value (Any-method-call RFC 20260704 C1+C2).
//!
//! ssa-lower interns the compile-time method name into an
//! `ANY_METHOD_*` id (torajs-rc), boxes each argument into a
//! stack-allocated argv of NaN-box AnyValues, and calls here; the
//! name bytes travel only for TypeError messages. Dispatch:
//!
//! - `null` / `undefined` receiver → catchable TypeError (ES
//!   §13.3.2 RequireObjectCoercible).
//! - ShortStr → `toString` is identity (immediates have copy
//!   semantics, so the bits return as-is — the materialize path
//!   would hand back a dropped temp); everything else materializes
//!   to a heap Str, reuses the Str arm, and drops the temp.
//! - `true` / `false` immediates → `toString` ("true"/"false" per
//!   ES §20.3.3.3); every other id is a TypeError.
//! - `Tag::Str` cell (Str or Substr view) → `toString` is identity
//!   (rc_inc the same cell, the caller owns the new +1); charAt /
//!   case / indexOf / includes / slice / split / trim glue
//!   (torajs-str `method_any`).
//! - `Tag::Arr` cell → push / pop / shift / unshift (torajs-arr
//!   `method_any`; growth-relocating methods write the possibly-
//!   moved receiver back through `recv_slot`) + indexOf / includes
//!   / join (`method_any_search`) + map / filter / forEach
//!   (`method_any_hof`, C3b — the callback resolves through
//!   `closure_boxed_entry` and the loop runs runtime-side); the
//!   id-switch lives in `method_call_arr`.
//! - `Tag::Closure` cell (chunk 710) → `Function.prototype.call` /
//!   `apply` with the thisArg dropped (a torajs closure body cannot
//!   reference `this`) + `bind` (chunk 714, `method_bind`) + expando
//!   shadowing (`method_call_closure`).
//! - `Tag::DynObj` cell (C3a-2) → probe the property by the interned
//!   name Str the lowerer now passes; a closure-cell value with a
//!   non-zero boxed dual entry (`+32`, synthesized per lifted body)
//!   invokes through the uniform `(env, argv, argc) -> AnyValue`
//!   ABI — argv rides in a fixed 8-slot undefined-filled buffer so
//!   the adapter reads its param count unconditionally
//!   (`method_call_dynobj`).
//! - `Tag::Obj` cell (L3b #9) → static-layout field probe through
//!   the class-layouts metadata; a `Closure` / closure-bearing `Any`
//!   slot invokes the same uniform ABI (`method_call_dynobj`).
//! - `Tag::Map` / `Tag::Set` cell (C4) → get / set / has / delete /
//!   add / clear / forEach in `method_call_mapset` (pair-ABI kernels
//!   + the C4-2 boxed-entry forEach walk) + the keys / values /
//!   entries iterator mints.
//! - `Tag::MapIter` cell → iterator-protocol `next()` (IteratorResult
//!   `{ value, done }` dynobj), same module.
//! - `Tag::Date` cell (C4-3a) → the full typed-tier method table
//!   (getters / to*String / setters) in `method_call_date`.
//! - int32 / double immediates (C4-3b) → toString / toFixed /
//!   toExponential / toPrecision / toLocaleString / valueOf in
//!   `method_call_num` (i / f kernel split off the box encoding).
//! - `Tag::RegExp` cell (C4-3c) → test / exec / toString in
//!   `method_call_regexp`.
//! - anything else (numeric immediates, other heap tags, unknown
//!   method ids) → catchable TypeError — the RFC's C3+ tags land
//!   here one arm at a time, never a silent wrong answer.
//!
//! Argument ledger: argv slots are BORROWED (the lowerer rc-decs
//! each one after the call); per-method glue incs what it keeps.
//! String-typed arguments materialize through `anyv_to_str` as
//! owned temps this dispatcher drops before returning. The returned
//! AnyValue follows the boxed-value convention (cells +1, owned by
//! the caller).

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ANY_METHOD_HAS_OWN_PROPERTY, ANY_METHOD_PROPERTY_IS_ENUMERABLE,
    ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF, Tag,
};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_null,
    is_short_str, is_true, is_undefined,
};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use crate::nanbox_ffi::__torajs_anyv_to_number;
use crate::nanbox_ffi_materialize::materialize_short_str;

// Closure-face dispatch primitives live in
// `method_call_closure_dispatch` to keep this file under the 500-line
// project cap. Re-export the pub(crate) surface so existing callers
// keep importing from `crate::method_call::{...}` unchanged.
pub(crate) use crate::method_call_closure_dispatch::{
    MAX_BOXED_ARGS, closure_boxed_entry, closure_cell_entry, invoke_boxed, invoke_boxed_recv_first,
    invoke_with_this, recv_first_shift,
};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-str — allocate a fresh Str from raw bytes (the bool
    /// arm's "true"/"false").
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-meta — [[GetPrototypeOf]] over any receiver (owned).
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    /// torajs-meta — Annex B §B.2.2.1.2 setter kernel.
    fn __torajs_anyv_proto_member_set(obj: u64, proto: u64);
}

/// `ToIntegerOrInfinity`-shaped argument decode: `undefined` (or a
/// missing slot) answers `default`; NaN answers 0; otherwise the
/// f64 truncates toward zero.
pub(crate) unsafe fn to_index(av: AnyValue, default: i64) -> i64 {
    if is_undefined(av) {
        return default;
    }
    let n = unsafe { __torajs_anyv_to_number(av) };
    if n.is_nan() { 0 } else { n as i64 }
}

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `argv` points at `argc`
/// AnyValue slots the caller keeps alive across the call;
/// `recv_slot` is NULL or the receiver variable's live slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_call(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    _name_len: i64,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let r = unsafe { any_method_call_inner(recv, mid, name_str, recv_slot, argv, argc) };
    if r == ANY_METHOD_NO_SUCH {
        return unsafe { not_callable() };
    }
    r
}

/// `o.m?.(args…)` flavor of the dispatcher (chunk 709) — a method
/// name the receiver doesn't have answers undefined instead of the
/// TypeError (ES §13.3.9 short-circuit on a nullish `o.m`); every
/// resolved-but-not-callable shape still throws.
///
/// # Safety
/// Same contract as [`__torajs_any_method_call`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_call_opt(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let r = unsafe { any_method_call_inner(recv, mid, name_str, recv_slot, argv, argc) };
    if r == ANY_METHOD_NO_SUCH {
        return VALUE_UNDEFINED;
    }
    r
}

/// Shared dispatch body — a mid-miss floats [`ANY_METHOD_NO_SUCH`]
/// to the two extern exits above. pub(crate): the reified-method
/// `call` / `apply` short-circuit re-enters here with the thisArg
/// as the receiver (chunk 711).
pub(crate) unsafe fn any_method_call_inner(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    // §20.1.3.6 Object.prototype.toString — the badge classifier
    // dispatches on EVERY this-value including undefined / null
    // (steps 1-2 answer their badges, no ToObject throw), so the
    // arm sits before the nullish guard. Reached only through the
    // reified badge cell (proto alias / `.call` re-dispatch /
    // expando-stored cell) — a plain `toString` name never interns
    // to this mid.
    if mid == torajs_rc::ANY_METHOD_OBJECT_TO_STRING {
        return unsafe { crate::method_call_object_proto::object_proto_to_string(recv) };
    }
    // %ThrowTypeError% (§10.2.4) — every invocation throws, whatever
    // the receiver, so the arm sits before the nullish guard (a
    // `.call(undefined)` must raise THIS TypeError, not the
    // method-of-nullish one).
    if mid == torajs_rc::ANY_METHOD_THROW_TYPE_ERROR {
        unsafe {
            __torajs_throw_type_error(
                c"'caller', 'callee', and 'arguments' properties may not be accessed".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    }
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot call a method of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    // chunk D-1 (RFC 20260711) — universal own-property probes
    // (§20.1.4.3 / §20.1.4.5): every receiver shape answers through
    // the prop_has substrate, so these dispatch BEFORE the per-tag
    // arms.
    if mid == ANY_METHOD_HAS_OWN_PROPERTY || mid == ANY_METHOD_PROPERTY_IS_ENUMERABLE {
        return unsafe { crate::method_call_object_proto::own_prop_probe(recv, mid, argv, argc) };
    }
    // §20.1.3.3 — universal chain-walk probe (knife 4).
    if mid == torajs_rc::ANY_METHOD_IS_PROTOTYPE_OF {
        return unsafe { crate::method_call_object_proto::is_prototype_of(recv, argv, argc) };
    }
    // Annex B §B.2.2.1 — the reified `get __proto__` / `set
    // __proto__` faces (RFC 20260718-accessor-reify 刀 1). The
    // nullish guard above IS the abrupt case (get-to-obj-abrupt /
    // set-non-obj-coercible); every remaining receiver routes to
    // the meta substrate — get answers the [[Prototype]] (owned,
    // primitives answer their wrapper prototype), set runs the
    // silent-invalid / refusal-throws Annex B semantics and
    // answers undefined.
    if mid == torajs_rc::ANY_METHOD_PROTO_GET {
        return unsafe { __torajs_anyv_get_proto_of_any(recv) };
    }
    if mid == torajs_rc::ANY_METHOD_PROTO_SET {
        let v = if argc >= 1 {
            unsafe { *argv }
        } else {
            VALUE_UNDEFINED
        };
        unsafe { __torajs_anyv_proto_member_set(recv, v) };
        return VALUE_UNDEFINED;
    }
    // Annex B §B.2.2.2-5 legacy accessor surface — universal like the
    // own-property probes (ToObject semantics per receiver shape live
    // in the arm).
    if (torajs_rc::ANY_METHOD_DEFINE_GETTER..=torajs_rc::ANY_METHOD_LOOKUP_SETTER).contains(&mid) {
        return unsafe {
            crate::method_call_legacy_accessor::legacy_accessor_method(
                recv, mid, recv_slot, argv, argc,
            )
        };
    }
    if is_short_str(recv) {
        // toString on a string is identity; a ShortStr is an
        // immediate (copy semantics, no rc), so the bits return
        // as-is — the materialize path below would hand back a
        // temp this dispatcher is about to drop. valueOf
        // (§22.1.3.35) and toLocaleString (§22.1.3.26 — plain
        // toString under the typed tier's locale posture) are the
        // same identity.
        if mid == ANY_METHOD_TO_STRING
            || mid == ANY_METHOD_VALUE_OF
            || mid == ANY_METHOD_TO_LOCALE_STRING
        {
            return recv;
        }
        // Materialize once, reuse the heap-Str arm, drop the temp
        // (results copy out of the temp's bytes, never alias it).
        unsafe {
            let tmp = materialize_short_str(recv);
            let out = crate::method_call_str::str_method(tmp, mid, argv, argc);
            __torajs_str_drop(tmp as *mut c_void);
            return out;
        }
    }
    if is_bool(recv) {
        return unsafe { bool_method(recv, mid, argv, argc) };
    }
    if is_int32(recv) || is_double(recv) {
        return unsafe { crate::method_call_num::number_method(recv, mid, argv, argc) };
    }
    if is_cell(recv) {
        let ptr = as_void_ptr(recv);
        let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
        // ES own-property order — a wrapper EXPANDO entry wins over
        // the view-through surface (`__instance.charAt =
        // String.prototype.charAt` stores the reified cell; the call
        // must run it against the wrapper receiver — the §22.1.3
        // generic ToString(this) coerce — not fall to the inner
        // primitive's arm, which knows no such method).
        if crate::member_get::is_wrapper_tag(tag)
            && let Some(out) = unsafe {
                crate::method_call_wrapper_expando::wrapper_expando_method(
                    recv, ptr, mid, name_str, argv, argc,
                )
            }
        {
            return out;
        }
        // RFC 20260716 刀 3 view-through (`wrapper_view_through`).
        if let Some(inner) = unsafe { crate::wrapper_view_through::resolve_inner_recv(ptr, tag) } {
            return unsafe { any_method_call_inner(inner, mid, name_str, recv_slot, argv, argc) };
        }
        // §20.1.4.7 Object.prototype.valueOf is ToObject(this) —
        // identity on every cell receiver. Date keeps its own
        // valueOf (the getTime alias in its per-tag arm), the
        // Number/Boolean immediates answered above, and the
        // property-carrying shapes (dynobj / struct) resolve their
        // OWN entry first — their arms fall back here-equivalent
        // via `object_proto_fallback` so a monkey-patch wins.
        if mid == ANY_METHOD_VALUE_OF
            && tag != Tag::Date as u16
            && tag != Tag::DynObj as u16
            && tag != Tag::Obj as u16
            && tag != Tag::Arr as u16
        {
            unsafe { __torajs_rc_inc(ptr) };
            return recv;
        }
        // §20.1.4.6 Object.prototype.toLocaleString invokes
        // this.toString — Str is identity, Arr delegates to the
        // §23.1.3.32 join-with-"," shape; plain objects answer
        // through their arm's own-probe-then-fallback (monkey-patch
        // order). Date / Number keep their own per-arm
        // specializations; other tags fall through to their arm (a
        // miss stays the no-such TypeError).
        if mid == ANY_METHOD_TO_LOCALE_STRING {
            if tag == Tag::Str as u16 {
                unsafe { __torajs_rc_inc(ptr) };
                return recv;
            }
            if tag == Tag::Arr as u16 {
                // §23.1.3.32 — per-element toLocaleString invoke
                // (a plain join never dispatched the element hook).
                return unsafe { crate::arr_locale_string::arr_to_locale_string(ptr) };
            }
        }
        if tag == Tag::Str as u16 {
            // toString is identity — hand the caller its own +1 on
            // the same cell per the boxed-value convention.
            if mid == ANY_METHOD_TO_STRING {
                unsafe { __torajs_rc_inc(ptr) };
                return recv;
            }
            return unsafe { crate::method_call_str::str_method(ptr as *mut u8, mid, argv, argc) };
        }
        if tag == Tag::Arr as u16 {
            // §10.1.8.1 OrdinaryGet — an own arr-props expando entry
            // shadows the built-in method surface (`arr.getClass =
            // Object.prototype.toString; arr.getClass()` / a user
            // closure stored on the array). One props-NULL check on
            // the no-expando fast path (RFC 20260713 blade 2).
            if !name_str.is_null()
                && let Some(r) = unsafe {
                    crate::method_call_dynobj::arr_expando_method(
                        ptr, recv, name_str, recv_slot, argv, argc,
                    )
                }
            {
                return r;
            }
            // §20.1.4.7 Object.prototype.valueOf identity — moved
            // after the expando probe so a patched `arr.valueOf`
            // wins (the early cell-wide identity arm excludes Arr
            // for exactly this monkey-patch order).
            if mid == ANY_METHOD_VALUE_OF {
                unsafe { __torajs_rc_inc(ptr) };
                return recv;
            }
            return unsafe { crate::method_call_arr::arr_method(ptr, mid, recv_slot, argv, argc) };
        }
        if tag == Tag::DynObj as u16 {
            return unsafe {
                crate::method_call_dynobj::dynobj_method(ptr, mid, name_str, recv_slot, argv, argc)
            };
        }
        // L3b #9 (chunk 524) — static-layout struct receivers probe
        // the class-layouts field metadata instead of a dynobj table.
        if tag == Tag::Obj as u16 {
            return unsafe {
                crate::method_call_dynobj::struct_method(ptr, mid, name_str, argv, argc)
            };
        }
        if tag == Tag::Map as u16 || tag == Tag::Set as u16 {
            return unsafe {
                crate::method_call_mapset::map_set_method(
                    ptr,
                    tag == Tag::Set as u16,
                    mid,
                    argv,
                    argc,
                )
            };
        }
        if tag == Tag::MapIter as u16 {
            return unsafe { crate::method_call_mapset::map_iter_method(ptr, mid) };
        }
        if tag == Tag::Date as u16 {
            return unsafe { crate::method_call_date::date_method(ptr, mid, argv, argc) };
        }
        if tag == Tag::RegExp as u16 {
            return unsafe { crate::method_call_regexp::regexp_method(ptr, mid, argv, argc) };
        }
        // chunk 710 — Function.prototype.call / apply on closure
        // values (expando shadowing included).
        if tag == Tag::Closure as u16 {
            return unsafe {
                crate::method_call_closure::closure_method(ptr, mid, name_str, argv, argc)
            };
        }
        // RC-2b (RFC 20260706-test262-bug-corpus) — WeakMap / WeakSet.
        if tag == Tag::WeakMap as u16 || tag == Tag::WeakSet as u16 {
            return unsafe {
                crate::method_call_weak::weak_method(
                    ptr,
                    tag == Tag::WeakSet as u16,
                    mid,
                    argv,
                    argc,
                )
            };
        }
    }
    unsafe {
        __torajs_throw_type_error(c"value is not a function on this any receiver".as_ptr());
    }
    VALUE_UNDEFINED
}

/// bool-immediate arm — `toString` / the inherited
/// `Object.prototype.toLocaleString` answer a fresh "true"/"false"
/// Str (ES §20.3.3.3), `valueOf` (§20.3.3.4) is the immediate
/// itself; 刀 2 (RFC 20260714-t262-top-clusters) — a generic
/// array-family mid reached through
/// `Array.prototype.every.call(true, …)` runs the empty-receiver
/// semantics (ToObject(bool) has no own `length` → every loop is
/// vacuous). Every other id is a TypeError.
unsafe fn bool_method(recv: AnyValue, mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    if mid == ANY_METHOD_VALUE_OF {
        return recv;
    }
    if mid == ANY_METHOD_TO_STRING || mid == ANY_METHOD_TO_LOCALE_STRING {
        let bytes: &[u8] = if is_true(recv) { b"true" } else { b"false" };
        unsafe {
            let p = __torajs_str_alloc(bytes.as_ptr(), bytes.len() as i64);
            return __torajs_anyv_box_pointer(p as *mut c_void);
        }
    }
    if crate::method_call_arraylike::arraylike_supported(mid)
        && !crate::method_call_arraylike_mut::arraylike_mut_supported(mid)
    {
        return unsafe { crate::method_call_arraylike::arraylike_empty(mid, argv, argc) };
    }
    unsafe { method_no_such() }
}

/// No-such-method sentinel — an impossible AnyValue bit pattern
/// (same reserved quiet-NaN corner as [`ANY_METHOD_THREW`]). A
/// per-arm mid-miss returns it up the dispatch chain; the extern
/// exits decide: [`__torajs_any_method_call`] throws (the pre-709
/// semantics), [`__torajs_any_method_call_opt`] answers undefined
/// (`o.m?.()` short-circuit for a method the receiver doesn't have).
pub(crate) const ANY_METHOD_NO_SUCH: u64 = u64::MAX - 1;

/// A method NAME the receiver's arm doesn't know — floats the
/// [`ANY_METHOD_NO_SUCH`] sentinel to the extern exit (which throws
/// or answers undefined per the opt flavor). Only for name misses;
/// a resolved-but-not-callable value is [`not_callable`].
pub(crate) unsafe fn method_no_such() -> AnyValue {
    ANY_METHOD_NO_SUCH
}

/// A value that resolved but cannot be invoked (non-closure
/// property, non-fn callback arg, non-callable bare callee) — a
/// definite catchable TypeError in both call flavors.
pub(crate) unsafe fn not_callable() -> AnyValue {
    unsafe {
        __torajs_throw_type_error(c"value is not a function on this any receiver".as_ptr());
    }
    VALUE_UNDEFINED
}
