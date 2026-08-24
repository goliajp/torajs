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
    ANY_METHOD_HAS_OWN_PROPERTY, ANY_METHOD_PROPERTY_IS_ENUMERABLE, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF,
};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, is_bool, is_cell, is_double, is_int32, is_null, is_short_str,
    is_undefined,
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
    /// torajs-meta — §20.5.3.4 generic Error.prototype.toString
    /// (owned Str out; NULL = pending throw recorded).
    fn __torajs_error_proto_to_string(recv: u64) -> *mut u8;
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
        // A per-arm mid-miss skipped the dispatch tail — give the
        // builtin-proto patch consult its §10.1.9.2 chain step
        // before the TypeError (RFC 20260721 刀 3).
        if let Some(out) = unsafe {
            crate::method_call_proto_patch::builtin_proto_patch_method(
                recv, mid, name_str, argv, argc,
            )
        } {
            return out;
        }
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
        // Same chain step as the throwing flavor — a live patch
        // resolves; only a true miss keeps the `?.()` undefined.
        if let Some(out) = unsafe {
            crate::method_call_proto_patch::builtin_proto_patch_method(
                recv, mid, name_str, argv, argc,
            )
        } {
            return out;
        }
        return VALUE_UNDEFINED;
    }
    r
}

// The dispatch SEAM (RFC 20260824-s2-5 Phase B blade 0): the body
// lives behind a C-ABI symbol resolved at LINK time — normally the
// thin `torajs-dispatch` archive member forwarding to
// [`any_method_dispatch_impl`], but a compiler-emitted specialized
// dispatcher in the user `.o` shadows it (user definitions win in
// the member closure), and the monolithic impl then strips.
unsafe extern "C" {
    fn __torajs_any_method_dispatch(
        recv: AnyValue,
        mid: i64,
        name_str: *const u8,
        recv_slot: *mut u64,
        argv: *const u64,
        argc: i64,
        skip_wrapper_expando: bool,
    ) -> AnyValue;
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
    unsafe { __torajs_any_method_dispatch(recv, mid, name_str, recv_slot, argv, argc, false) }
}

/// Re-dispatch entry for an invoked reified-builtin cell — the
/// cell's [[Call]] IS the prototype method body, and the ordinary
/// method lookup already resolved to it, so the body must NOT
/// consult the receiver's own properties again: a wrapper expando
/// storing the same-mid cell under the same name would re-resolve
/// to itself forever (the S15.6.4.2_A2 stack-overflow family).
pub(crate) unsafe fn any_method_redispatch(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        __torajs_any_method_dispatch(
            recv,
            mid,
            core::ptr::null(),
            core::ptr::null_mut(),
            argv,
            argc,
            true,
        )
    }
}

/// The dispatch body behind the two entries above (reached through
/// the `__torajs_any_method_dispatch` link seam) —
/// `skip_wrapper_expando` marks a reified-builtin re-dispatch
/// (method body execution; own-property probing is over).
///
/// # Safety
/// Same contract as [`__torajs_any_method_call`]; `argv` holds
/// `argc` live AnyValue slots.
pub unsafe fn any_method_dispatch_impl(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
    skip_wrapper_expando: bool,
) -> AnyValue {
    // §7.3.2 — `p.m()` on a Proxy is `Call(GetV(p, "m"), p, args)`,
    // so the NAME goes through [[Get]] before anything shape-keyed
    // gets a say (RFC 20260823-proxy-substrate 刀 1). Gated off
    // re-dispatch by the same flag the monkey-patch consult uses: a
    // reified builtin's body must not re-resolve.
    if !skip_wrapper_expando && !name_str.is_null() && crate::proxy::is_proxy(recv) {
        return unsafe {
            crate::proxy_call::method_call(recv, mid, name_str, recv_slot, argv, argc)
        };
    }
    if let Some(v) = unsafe { crate::method_call_prelude::pre_nullish_arm(recv, mid, argv, argc) } {
        return v;
    }
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot call a method of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    // §23.1.3.36 — the reified `Array.prototype.toString` cell
    // borrowed across receivers (RFC 20260721 刀 11 G12): join for
    // an Array, `Get(this, "join")` else, badge fallback. Sits after
    // the nullish guard (step 1 is ToObject).
    if mid == torajs_rc::ANY_METHOD_ARR_TO_STRING {
        return unsafe { crate::method_call_object_proto::arr_to_string_borrowed(recv) };
    }
    // §21.4.4.37 — the reified `Date.prototype.toJSON` cell's
    // [[Call]] body is receiver-generic (ToPrimitive number
    // non-finite → null, else Invoke toISOString). Redispatch-only:
    // a plain-named `obj.toJSON()` keeps ordinary own-property
    // routing (a user object's own `toJSON` must win).
    if mid == torajs_rc::ANY_METHOD_TO_JSON && skip_wrapper_expando {
        return unsafe { crate::method_call_date::date_to_json_generic(recv) };
    }
    // chunk D-1 (RFC 20260711) — universal own-property probes
    // (§20.1.4.3 / §20.1.4.5): every receiver shape answers through
    // the prop_has substrate, so these dispatch BEFORE the per-tag
    // arms.
    if mid == ANY_METHOD_HAS_OWN_PROPERTY || mid == ANY_METHOD_PROPERTY_IS_ENUMERABLE {
        return unsafe { crate::method_call_object_proto::own_prop_probe(recv, mid, argv, argc) };
    }
    // §20.4.3.3 / §20.4.3.4 — the reified Symbol.prototype.toString
    // / valueOf cells: thisSymbolValue throws a TypeError on every
    // non-Symbol receiver (`Symbol.prototype.valueOf.call(0)` must
    // not answer 0 through the number arm). Sits after the nullish
    // guard — its TypeError covers the null/undefined legs.
    if mid == torajs_rc::ANY_METHOD_SYMBOL_TO_STRING || mid == torajs_rc::ANY_METHOD_SYMBOL_VALUE_OF
    {
        return unsafe { crate::method_call_cell::symbol_proto_method(recv, mid) };
    }
    // §20.4.3.2 — the reified `get Symbol.prototype.description`
    // getter cell: thisSymbolValue gate, then the [[Description]]
    // Str (undefined for `Symbol()`). Only reachable through the
    // carried-mid re-dispatch (the id never interns).
    if mid == torajs_rc::ANY_METHOD_GET_DESCRIPTION {
        return unsafe { crate::method_call_cell::symbol_description_getter(recv) };
    }
    // §22.1.3.36 — the reified `String.prototype[Symbol.iterator]`
    // cell (F0 string leg): receiver-generic ToString(this), then a
    // VALUES ArrIter over the character array. The id never interns;
    // the nullish guard above is RequireObjectCoercible.
    if mid == torajs_rc::ANY_METHOD_STR_ITERATOR {
        return unsafe { crate::str_iterator::str_iterator_mint(recv) };
    }
    // §20.5.3.4 — the dedicated Error.prototype.toString cell:
    // generic Get(name)/Get(message) steps over any object receiver
    // (FLAG_ERROR instances ride the fixed-offset fast lane inside).
    // NULL answer = the helper recorded a pending throw (non-object
    // receiver / abrupt Get / abrupt ToString).
    if mid == torajs_rc::ANY_METHOD_ERROR_TO_STRING {
        let s = unsafe { __torajs_error_proto_to_string(recv) };
        if s.is_null() {
            return VALUE_UNDEFINED;
        }
        return unsafe { __torajs_anyv_box_pointer(s as *mut c_void) };
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
    // RFC 20260721 刀 11 G13 — the primitive fast arms below answer
    // their mids natively, so a builtin-prototype monkey-patch must
    // consult FIRST; the (tag, mid) patch bitmap keeps the no-patch
    // program at one relaxed load. Gated off re-dispatch like the
    // tail consult (reified-builtin body execution never re-resolves).
    if !skip_wrapper_expando
        && let Some(out) = unsafe {
            crate::method_call_proto_patch::primitive_patch_pregate(recv, mid, name_str, argv, argc)
        }
    {
        return out;
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
            let boxed = crate::nanbox_encode::__torajs_anyv_box_pointer(tmp as *mut c_void);
            let out = crate::dispatch_seam::__torajs_dispatch_str_arm(
                boxed, mid, name_str, recv_slot, argv, argc,
            );
            __torajs_str_drop(tmp as *mut c_void);
            return out;
        }
    }
    if is_bool(recv) {
        return unsafe { crate::method_call_bool::bool_method(recv, mid, argv, argc) };
    }
    if is_int32(recv) || is_double(recv) {
        return unsafe {
            crate::dispatch_seam::__torajs_dispatch_num_arm(
                recv, mid, name_str, recv_slot, argv, argc,
            )
        };
    }
    if is_cell(recv)
        && let Some(out) = unsafe {
            crate::method_call_cell::cell_method_inheriting(
                recv,
                mid,
                name_str,
                recv_slot,
                argv,
                argc,
                skip_wrapper_expando,
            )
        }
    {
        return out;
    }
    // Builtin-prototype monkey-patch consult (RFC 20260721 刀 3) —
    // gated off re-dispatch: a reified-builtin body execution must
    // not re-resolve (cycle posture, see the module).
    if !skip_wrapper_expando
        && let Some(out) = unsafe {
            crate::method_call_proto_patch::builtin_proto_patch_method(
                recv, mid, name_str, argv, argc,
            )
        }
    {
        return out;
    }
    unsafe {
        __torajs_throw_type_error(c"value is not a function on this any receiver".as_ptr());
    }
    VALUE_UNDEFINED
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
