//! §23.1.3.26 `Array.prototype.reverse` MOP slow path — RFC
//! 20260721-array-proto-cluster 刀 13d.
//!
//! The raw 8-byte slot swap in `transform.rs::__torajs_arr_reverse`
//! is only valid for a clean receiver. An exotic `Array<Any>`
//! (accessor indexes / hole shadows / attribute shadows) must ride
//! the spec's per-pair four-step MOP in exact low→high access order
//! — HasProperty(lower) / Get(lower) / HasProperty(upper) /
//! Get(upper), then the exists-shaped Set / DeletePropertyOrThrow
//! writes — because getter and setter side effects, hole delete
//! semantics, and prototype digit-key reads are all observable
//! (test262 reverse/get_if_present_with_delete +
//! staging/sm reverse-order-of-low-high-accesses).
//!
//! `len` is the §23.1.3.26 step 2 LengthOfArrayLike snapshot; every
//! HasProperty / Get afterwards reads live (a getter shrinking the
//! array mid-loop makes later indexes read through the prototype
//! chain, matching spec re-evaluation).

use core::ffi::c_void;

use crate::define::mint_index_key;
use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    /// torajs-anyvalue — §7.3.11 HasProperty walk (live index +
    /// hole shadows + expando + prototype chain), borrow-box recv.
    fn __torajs_arr_forin_key_live(arr: *mut c_void, key: *const c_void) -> i64;
    /// torajs-anyvalue — NaN-box field reads (mirror `any.rs`).
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — non-destructive pending-throw probe.
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// HasProperty(O, ToString(idx)) — mint the canonical index key,
/// walk, drop the key.
unsafe fn has_property(arr: *mut c_void, idx: u64) -> bool {
    unsafe {
        let key = mint_index_key(idx);
        let live = __torajs_arr_forin_key_live(arr, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        live != 0
    }
}

/// Set(O, idx, v, true) — route the owned boxed value through the
/// kind-aware any set kernel (setter invoke / slot store + hole
/// revive); the slot adopts the ref, mirroring the emit-side write
/// ledger.
unsafe fn set_index(arr: *mut c_void, idx: u64, av: u64) {
    unsafe {
        let tag = __torajs_anyv_unbox_tag(av) as u64;
        let value = __torajs_anyv_unbox_value(av) as u64;
        crate::any::__torajs_arr_set_any(arr, idx, tag, value);
    }
}

/// DeletePropertyOrThrow(O, ToString(idx)) — §7.3.10: a refused
/// delete (non-configurable index) raises TypeError.
unsafe fn delete_or_throw(arr: *mut c_void, idx: u64) {
    unsafe {
        let key = mint_index_key(idx);
        let ok = crate::define_hole::__torajs_arr_delete_index(arr, key as *mut c_void, idx);
        __torajs_str_drop(key as *mut c_void);
        if ok == 0 {
            __torajs_throw_type_error(c"Unable to delete property.".as_ptr());
        }
    }
}

/// The §23.1.3.26 step 3-5 pair loop. Returns the receiver pointer
/// for chaining (same contract as the raw-swap fast path). Aborts
/// on the first pending throw (getter / setter / refused delete).
pub(crate) unsafe fn reverse_mop(arr: *mut u8) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — ~len/2 Has/Get/Set rounds over the
        // unmaterialized tail; loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.reverse\0".as_ptr(),
        ) {
            return arr;
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let middle = len / 2;
        let recv = arr as *mut c_void;
        for lower in 0..middle {
            let upper = len - lower - 1;
            let lower_exists = has_property(recv, lower);
            let lower_val = if lower_exists {
                crate::index_any::__torajs_arr_index_get(recv as *const c_void, lower as i64)
            } else {
                0
            };
            if __torajs_throw_check() != 0 {
                return arr;
            }
            let upper_exists = has_property(recv, upper);
            let upper_val = if upper_exists {
                crate::index_any::__torajs_arr_index_get(recv as *const c_void, upper as i64)
            } else {
                0
            };
            if __torajs_throw_check() != 0 {
                return arr;
            }
            match (lower_exists, upper_exists) {
                (true, true) => {
                    set_index(recv, lower, upper_val);
                    set_index(recv, upper, lower_val);
                }
                (false, true) => {
                    set_index(recv, lower, upper_val);
                    delete_or_throw(recv, upper);
                }
                (true, false) => {
                    delete_or_throw(recv, lower);
                    set_index(recv, upper, lower_val);
                }
                (false, false) => {}
            }
            if __torajs_throw_check() != 0 {
                return arr;
            }
        }
        arr
    }
}
