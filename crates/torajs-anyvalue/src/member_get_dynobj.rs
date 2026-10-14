//! The `Tag::DynObj` receiver's read, both channels: the own probe
//! that answers the whole `(tag, value)` pair at once, and what an
//! ordinary object answers for a key it does not own (the Annex B
//! `__proto__` accessor, the user prototype chain, a null prototype,
//! the globalThis fill gaps, the builtin prototype faces and the
//! reify tail). The two tails live side by side so their arm-for-arm
//! order is read in one place.
//!
//! [`__torajs_any_member_get_pair`] is the one-call read the lowering
//! emits for a dynamic key: an ordinary object that owns the key
//! answers both channels from one probe; an absent key and every other
//! receiver get the two channels of `member_get.rs` /
//! `member_get_value.rs` in their usual order.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{__torajs_any_member_get_tag, recv_cell};
use crate::member_get_own::{dynobj_proto_pair, user_proto_cell};
use crate::member_get_reify_tail::dynobj_builtin_tail_tag;
use crate::member_get_value::{__torajs_any_member_get_value, reify_value};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-dynobj — the own entry's `(tag, value)` from one probe
    /// (a null `value_out` takes the tag alone);
    /// [`DYNOBJ_GET_OWN_MISS`] when there is no entry.
    fn __torajs_dynobj_get_own(obj: *const c_void, key: *const c_void, value_out: *mut u64) -> u64;
}

/// `torajs_dynobj::get::DYNOBJ_GET_OWN_MISS` mirror.
const DYNOBJ_GET_OWN_MISS: u64 = u64::MAX;

/// The tag the receiver's own entry for `key` answers (an entry
/// storing `undefined` is `Some(5)`); `None` when the receiver owns
/// no entry for it. The value is not decoded, so a ShortStr entry
/// materializes nothing.
///
/// # Safety
/// `ptr` is a live DynObj cell; `key` is a live Str or Symbol cell.
#[inline]
pub(crate) unsafe fn dynobj_own_tag(ptr: *mut c_void, key: *const c_void) -> Option<u64> {
    let tag = unsafe { __torajs_dynobj_get_own(ptr, key, core::ptr::null_mut()) };
    (tag != DYNOBJ_GET_OWN_MISS).then_some(tag)
}

/// The receiver's own entry for `key` as its whole `(tag, value)`
/// pair (an entry storing `undefined` is `Some((5, 0))`; a ShortStr
/// value is a fresh `Str` the caller owns); `None` when the receiver
/// owns no entry for it, a NULL or non-DynObj cell included.
///
/// # Safety
/// `ptr` is NULL or a live cell; `key` is a live Str or Symbol cell.
#[inline]
pub(crate) unsafe fn dynobj_own_pair(ptr: *const c_void, key: *const c_void) -> Option<(u64, u64)> {
    let mut value = 0u64;
    let tag = unsafe { __torajs_dynobj_get_own(ptr, key, &mut value) };
    (tag != DYNOBJ_GET_OWN_MISS).then_some((tag, value))
}

/// Both channels of an any-lane member read in one call: the tag is
/// returned and the value written to `value_out`, each exactly what
/// `__torajs_any_member_get_tag` / `__torajs_any_member_get_value`
/// answer for the same `(recv, key)` (accessor sentinel, absent
/// `(5, 0)` and the null / undefined receiver TypeError included).
///
/// # Safety
/// As `__torajs_any_member_get_tag`; `value_out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_pair(
    recv: AnyValue,
    key: *const c_void,
    value_out: *mut u64,
) -> u64 {
    unsafe {
        // An own entry answers a Str and a Symbol key alike (§10.1.8.1
        // OrdinaryGet step 2 stops there for either key kind).
        if let Some((ptr, t)) = recv_cell(recv)
            && t == Tag::DynObj as u16
        {
            // a miss leaves `value_out` alone
            let tag = __torajs_dynobj_get_own(ptr, key, value_out);
            if tag != DYNOBJ_GET_OWN_MISS {
                return tag;
            }
        }
        // An absent key and every other receiver take the two
        // channels, so each absent-key tail keeps a single caller (and
        // its inlined place in that channel).
        let tag = __torajs_any_member_get_tag(recv, key);
        value_out.write(__torajs_any_member_get_value(recv, key));
        tag
    }
}

/// Tag channel of an absent key; the order inside is load-bearing and
/// is documented step by step where each step sits.
///
/// # Safety
/// `ptr` is a live DynObj cell that owns no entry for `key`; `key` is
/// NULL or a live Str cell; `recv` boxes `ptr`.
#[inline]
pub(crate) unsafe fn dynobj_miss_tag(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
        // Annex B §B.2.2.1 — a dynamic key spelling `__proto__`
        // answers the RECEIVER's [[Prototype]] via the inherited
        // accessor (the own DATA probe above already covered the
        // shadow case); the chain walk below must not run — the
        // internal simulation slot no longer carries the
        // user-spellable name, so the walk would miss to the
        // reify surface.
        if crate::prop_has::key_is(key, b"__proto__") {
            return dynobj_proto_pair(ptr).0;
        }
        // Knife 2 — the user [[Prototype]] chain answers before
        // the builtin surface reifies (§10.1.8.1 OrdinaryGet
        // walks the chain; the recursion covers grandparents).
        if let Some(parent) = user_proto_cell(ptr) {
            return __torajs_any_member_get_tag(parent, key);
        }
        // §10.1.8.1 OrdinaryGet step 2 — an explicit null proto
        // cuts the chain: no builtin reify surface either.
        if crate::member_get_own::dynobj_null_proto(ptr) {
            return 5;
        }
        // RFC 20260807-global-object G2 — a KNOWN builtin the
        // globalThis singleton's fill list is missing must stay
        // LOUD (bun answers a function; a silent undefined here
        // would mask it). Unknown names keep the ordinary miss.
        // None of the missing names collides with the builtin
        // tails below, so probing first loses nothing.
        if crate::method_value::globalthis_object::globalthis_missing_known(ptr, key) {
            __torajs_throw_type_error(
                c"not yet supported: this builtin is not reachable through globalThis".as_ptr(),
            );
            return 5;
        }
        dynobj_builtin_tail_tag(ptr, recv, key)
    }
}

/// Value channel of an absent key — arm-for-arm twin of
/// [`dynobj_miss_tag`].
///
/// # Safety
/// As [`dynobj_miss_tag`].
#[inline]
pub(crate) unsafe fn dynobj_miss_value(
    ptr: *mut c_void,
    recv: AnyValue,
    key: *const c_void,
) -> u64 {
    unsafe {
        // Annex B §B.2.2.1 dynamic-key `__proto__` read —
        // see the tag twin.
        if crate::prop_has::key_is(key, b"__proto__") {
            return dynobj_proto_pair(ptr).1;
        }
        // Knife 2 — user chain before builtin reify
        // (tag twin above).
        if let Some(parent) = user_proto_cell(ptr) {
            return __torajs_any_member_get_value(parent, key);
        }
        // Explicit null proto cuts the chain — tag twin
        // above (§10.1.8.1 OrdinaryGet step 2).
        if crate::member_get_own::dynobj_null_proto(ptr) {
            return 0;
        }
        // G2 globalThis missing-known probe — tag twin
        // above (the tag channel already threw; answering 0
        // here keeps the pair protocol coherent).
        if crate::method_value::globalthis_object::globalthis_missing_known(ptr, key) {
            return 0;
        }
        let cell = crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key);
        if cell != 0 {
            return cell;
        }
        // Function.prototype's virtual own name/length pair
        // (§20.2.3, RFC 20260722 刀 3) — tag twin above.
        if let Some((_, mval)) = crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key)
        {
            return mval;
        }
        // Direct proto-accessor read (tag twin above) — the zero
        // value channel of the accessor sentinel.
        if crate::method_support_proto::proto_virtual_accessor_owned(ptr, key) {
            return 0;
        }
        // Implicit %Object.prototype% hop — tag twin above.
        if let Some(parent) = crate::member_get_own::implicit_proto_parent(ptr) {
            return __torajs_any_member_get_value(parent, key);
        }
        // Inherited Object.prototype reify (tag twin above).
        reify_value(recv, key)
    }
}
