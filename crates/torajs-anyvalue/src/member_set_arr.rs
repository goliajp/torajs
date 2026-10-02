//! `Tag::Arr` receiver arm of [`crate::member_set`] — RFC
//! 20260712-arr-exotic-define chunk C own-domain routing, so an index
//! key reaches element storage instead of landing in the expando
//! dynobj. Split out of `member_set.rs` (rotation 268 —
//! Reflect.set 参数化前的余量腾挪, mechanical move).

use core::ffi::c_void;

use torajs_rc::ANY_WPROP_ARR_LENGTH;

use crate::member_set::{__torajs_throw_type_error, drop_payload};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    /// torajs-arr — the arguments object's ordinary `length` write
    /// (0 = not an arguments object).
    fn __torajs_arr_arguments_length_set(
        arr: *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    ) -> i64;
    fn __torajs_throw_check() -> i64;
    /// torajs-arr — expando write through the lazily-allocated
    /// props dynobj.
    fn __torajs_arrprops_set(arr_ptr: *mut c_void, key: *const c_void, tag: i64, value: i64);
    /// torajs-arr — the same write in its Reflect.set flavor (a
    /// refusal answers 0, no TypeError recorded).
    fn __torajs_arrprops_set_soft(
        arr_ptr: *mut c_void,
        key: *const c_void,
        tag: i64,
        value: i64,
    ) -> i64;
    /// torajs-arr — own-key membership over that same bag.
    fn __torajs_arrprops_has(arr_ptr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-arr — §10.4.2.4 length setter (ToUint32 validate +
    /// real resize) for the dynamic-string-key `o[k] = v` form.
    fn __torajs_arr_set_length_any(arr: *mut c_void, tag: i64, value: i64);
    /// torajs-arr — the same write in its Reflect.set flavor (a
    /// refusal answers 0, no TypeError recorded).
    fn __torajs_arr_set_length_any_soft(arr: *mut c_void, tag: i64, value: i64) -> i64;
    /// torajs-arr — kind-aware element store (consumes a tag-4 rc).
    fn __torajs_arr_index_set(arr: *mut c_void, idx: i64, tag: u64, value: u64);
    /// torajs-arr — element store's growing twin (§10.4.2.1
    /// OrdinarySet past the end: reserve, hole-fill the gap, bump
    /// len). The cell never moves (grow.rs B1 — only the data
    /// buffer reallocates), so the returned pointer is `arr` itself
    /// and needs no write-back.
    fn __torajs_arr_set_any_grow(arr: *mut c_void, i: u64, tag: u64, value: u64) -> *mut u8;
    /// torajs-arr — per-index attribute flags (RFC
    /// 20260712-arr-exotic-define chunk C writable gate).
    fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64;
    /// torajs-arr — the index's AccessorPair, NULL when not an
    /// accessor (RFC 20260713 chunk C).
    fn __torajs_arr_index_accessor(arr: *const c_void, idx: u64) -> *mut c_void;
    /// torajs-dynobj — setter dispatch; `0` = getter-only accessor.
    fn __torajs_accessor_invoke_setter(pair: *const c_void, recv_anyv: u64, value_anyv: u64)
    -> i32;
    /// torajs-arr — re-create a deleted index as a default data
    /// property (hole revive, RFC 20260713 chunk C).
    fn __torajs_arr_index_revive(arr: *mut c_void, key: *mut c_void);
}

/// `Tag::Arr` receiver — RFC 20260712-arr-exotic-define chunk C
/// own-domain routing, so an index key reaches element storage
/// instead of landing in the expando dynobj. Flavored (R3-style):
/// refusals throw or answer 0 per `throw_on_refusal`.
///
/// # Safety
/// `ptr` is a live `Tag::Arr` cell; `key` is a live Str cell;
/// `(tag, value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn set_arr_member(
    ptr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    hint: i64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        // RFC 20260712-arr-exotic-define chunk C — own-domain
        // routing for the dynamic string key. Pre-fix every
        // `o[k] = v` landed in the expando dynobj: an index key
        // never reached element storage (the write "succeeded"
        // but reads answered the old element), and a "length"
        // key missed the resize path.
        if hint == ANY_WPROP_ARR_LENGTH || crate::prop_has::key_is(key, b"length") {
            // §10.4.4 — an arguments object's length is an ordinary data
            // property; ArraySetLength below is the Array's rule.
            if __torajs_arr_arguments_length_set(ptr, key, tag, value) != 0 {
                return (__torajs_throw_check() == 0) as i64;
            }
            // §28.1.13 Reflect.set — every refusal the kernel decides
            // (a non-writable length, a seal's shrink, a stopped shrink)
            // answers false through the soft flavor.
            if !throw_on_refusal {
                return __torajs_arr_set_length_any_soft(ptr, tag as i64, value as i64);
            }
            __torajs_arr_set_length_any(ptr, tag as i64, value as i64);
            return 1;
        }
        if let Some(idx) = crate::prop_has::canonical_index(key) {
            // An accessor index writes through its setter (RFC
            // 20260713 chunk C) — checked before the writable
            // gate, which would misread the pair entry's dead w
            // bit as a readonly data property.
            let pair = __torajs_arr_index_accessor(ptr, idx);
            if !pair.is_null() {
                let value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
                let recv_anyv = __torajs_anyv_box_from_pair(4, ptr as i64);
                if __torajs_accessor_invoke_setter(pair, recv_anyv, value_anyv) == 0 {
                    if throw_on_refusal {
                        __torajs_throw_type_error(
                            c"Attempted to assign to readonly property.".as_ptr(),
                        );
                    }
                    return 0;
                }
                return 1;
            }
            let flags = __torajs_arr_index_flags(ptr, idx);
            if flags & crate::prop_has::ARR_F_HOLE != 0 {
                // A deleted index is absent — the set re-creates
                // it as a fresh default data property (chunk C;
                // the shadow upsert clears the hole sentinel).
                __torajs_arr_index_set(ptr, idx as i64, tag, value);
                __torajs_arr_index_revive(ptr, key);
                return 1;
            }
            if flags & 0x1 == 0 {
                drop_payload(tag, value);
                if throw_on_refusal {
                    __torajs_throw_type_error(
                        c"Attempted to assign to readonly property.".as_ptr(),
                    );
                }
                return 0;
            }
            // Cluster #3 (rotation 442) — the growing store, not the
            // bounds-checked one: a canonical index key at or past
            // `len` CREATES the element per §10.4.2.1 OrdinarySet
            // (S15.4_A1.1_T4's `x["0"] = 0` on an empty array). The
            // flags probe above already answered writable/extensible
            // for the in-bounds and frozen shapes; in-bounds writes
            // behave exactly as `__torajs_arr_index_set` did.
            __torajs_arr_set_any_grow(ptr, idx, tag, value);
            return 1;
        }
        // §10.1.9.2 — an own miss consults the [[Prototype]] chain
        // before the own create. Ahead of the integrity gate: freezing
        // bars the create, not the walk.
        // The own faces above (index domain, `length`) have already
        // returned; what is left is the expando bag.
        let present = __torajs_arrprops_has(ptr, key) != 0;
        if !present
            && let Some(handled) = crate::member_set_dynobj::inherited_set_handled(
                ptr,
                crate::nanbox::box_void_ptr(ptr),
                key,
                tag,
                value,
                throw_on_refusal,
            )
        {
            return handled;
        }
        // `xs[0.5] = v` and `xs.foo = v` on a frozen / sealed /
        // non-extensible array both land here — before this the
        // expando was created regardless.
        if crate::member_set_closure::refuse_new_key_non_extensible(
            ptr,
            present,
            tag,
            value,
            throw_on_refusal,
        ) {
            return 0;
        }
        if !throw_on_refusal {
            return __torajs_arrprops_set_soft(ptr, key, tag as i64, value as i64);
        }
        __torajs_arrprops_set(ptr, key, tag as i64, value as i64);
        1
    }
}
