//! The shared IteratorResult [[Get]] — §7.4.4 IteratorComplete /
//! §7.4.5 IteratorValue read one property off a step object, whatever
//! its shape. Both step tiers (`iter_any::step_derived_iterator`,
//! `iter_any_get_method::generic_iter_step`) read through here.
//! Split out of `iter_any.rs` (file-size HARD RULE).

use core::ffi::c_void;

use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::payload_rc_inc;
use crate::struct_probe::struct_field_pair_bytes;

/// `torajs_rc::Tag::Obj` mirror — the struct fast lane's receiver
/// contract (see the gate below).
const TAG_OBJ_MIRROR: u16 = torajs_rc::Tag::Obj as u16;

unsafe extern "C" {
    /// torajs-str — mint a Str cell for the general-dispatcher lane
    /// (the member kernels key by Str cell).
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release the minted key.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — non-zero iff a throw is in flight (a getter the
    /// dispatcher lane invoked may throw).
    fn __torajs_throw_check() -> i64;
    /// torajs-dynobj — the [`iter_result_obj`] mint pair.
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set_fresh(dst: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
}

/// Fresh `{ value, done }` IteratorResult dynobj; `value` transfers
/// in owned (same ledger as the MapIter/ArrIter next() arms).
pub(crate) unsafe fn iter_result_obj(value: AnyValue, done: bool) -> AnyValue {
    unsafe {
        let mut obj = __torajs_dynobj_alloc();
        let (tag, payload) = (
            crate::__torajs_anyv_unbox_tag(value),
            crate::__torajs_anyv_unbox_value(value),
        );
        let k_value = __torajs_str_alloc(c"value".as_ptr() as *const u8, 5);
        __torajs_dynobj_set_fresh(&mut obj, k_value as *mut c_void, tag as u64, payload as u64);
        __torajs_str_drop(k_value as *mut c_void);
        let k_done = __torajs_str_alloc(c"done".as_ptr() as *const u8, 4);
        __torajs_dynobj_set_fresh(&mut obj, k_done as *mut c_void, 1, done as u64);
        __torajs_str_drop(k_done as *mut c_void);
        obj as u64
    }
}

/// One §7.4.4/§7.4.5 [[Get]] off the IteratorResult, whatever its
/// shape. The struct-layout probe answers a plain struct step (a
/// generator's, an object literal's) with zero allocation; a step
/// the layout walk cannot see — a dynobj (a defineProperty-degraded
/// object), an accessor property (test262's poisoned `value`
/// getter) — rides the general member dispatcher, the real [[Get]].
/// Before this the raw probe answered `None` for a dynobj step, so
/// `done` stayed false and `value` stayed undefined forever: the
/// poisoned getter never fired and the walk never terminated.
///
/// Answers the OWNED boxed value; `None` when a getter threw (the
/// pending throw is already recorded — the caller forwards it).
///
/// # Safety
/// `step` is a live cell whose pointer is `step_ptr`.
pub(crate) unsafe fn iter_result_get(
    step: AnyValue,
    step_ptr: *mut c_void,
    name: &[u8],
) -> Option<AnyValue> {
    unsafe {
        // The fast lane's `struct_field_raw` contract is a live
        // `Tag::Obj` pointer — its class-tag read sits at +8, which
        // on a DynObj step (the `iter_result_obj` mint every builtin
        // iterator answers) is a different header field entirely.
        // Ungated, that field's value was looked up as a CLASS TAG:
        // for years tags 1-4 always belonged to the injected Error
        // hierarchy (no `done`/`value` fields — miss, fall through,
        // correct), but the first program whose tag-1 class carries a
        // matching layout entry read a garbage `done` off itself and
        // every builtin-iterator spread answered [] (rotation-496
        // reachability-gated injection surfaced it; latent since the
        // fast lane landed).
        let step_heap_tag = (step_ptr.cast::<u8>().add(4) as *const u16).read();
        if step_heap_tag == TAG_OBJ_MIRROR
            && let Some((tag, payload)) = struct_field_pair_bytes(step_ptr, name)
        {
            payload_rc_inc(tag as i64, payload as i64);
            return Some(__torajs_anyv_box_from_pair(tag as i64, payload as i64));
        }
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64) as *const c_void;
        let tag = crate::member_get::__torajs_any_member_get_tag(step, key);
        let out = if tag == crate::struct_probe::ANY_ACCESSOR_TAG {
            let pair_bits = crate::member_get_value::__torajs_any_member_get_value(step, key);
            let got = crate::struct_probe::__torajs_any_accessor_get(step, key, pair_bits);
            if __torajs_throw_check() != 0 {
                __torajs_str_drop(key as *mut c_void);
                return None;
            }
            got
        } else {
            // The probe pair is a borrow off the receiver — convert
            // to owned before boxing, same as the struct lane above.
            let payload = crate::member_get_value::__torajs_any_member_get_value(step, key);
            payload_rc_inc(tag as i64, payload as i64);
            __torajs_anyv_box_from_pair(tag as i64, payload as i64)
        };
        __torajs_str_drop(key as *mut c_void);
        Some(out)
    }
}
