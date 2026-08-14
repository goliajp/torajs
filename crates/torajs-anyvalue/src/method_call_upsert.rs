//! `Map.prototype.getOrInsertComputed` (stage-3 upsert, 383-04) —
//! the callback-carrying half `__torajs_map_get_or_insert` cannot
//! express: a miss calls `callbackfn(key)` and inserts (and answers)
//! what it returns, and the spec's late re-set means the composition
//! `peek → call → set` IS the semantics — a callback that mutated
//! the map is overwritten by the post-callback set, exactly as
//! §Map.prototype.getOrInsertComputed steps it.
//!
//! Both lanes route here: the typed lowering calls the `no_mangle`
//! kernel, and `method_call_mapset`'s mid arm calls the core with
//! the same ownership shape — the KEY pair arrives owned (the
//! `pair_consumed` contract), the callback box is BORROWED, and the
//! answer is +1-owned for the caller.

use core::ffi::c_void;

use crate::method_call::{MAX_BOXED_ARGS, closure_boxed_entry, recv_first_shift};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

type BoxedFn = unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64;

unsafe extern "C" {
    fn __torajs_map_peek(
        p: *mut c_void,
        key_tag: i64,
        key_payload: i64,
        out_found: *mut i64,
        out_tag: *mut i64,
        out_payload: *mut i64,
    );
    fn __torajs_map_set(p: *mut c_void, kt: i64, kp: i64, vt: i64, vp: i64);
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Invoke a boxed callback with ONE argument and `this = undefined`
/// (the recv-first shift covers both adapter shapes). The return is
/// +1-owned; the caller checks the pending throw.
///
/// # Safety
/// `cb_env` / `cb_entry` are a live closure pair; `arg0` is borrowed
/// AnyValue bits the caller keeps alive across the call.
pub(crate) unsafe fn call_cb1(cb_env: *mut c_void, cb_entry: u64, arg0: u64) -> u64 {
    unsafe {
        let cb: BoxedFn = core::mem::transmute(cb_entry as usize);
        let s = recv_first_shift(cb_env);
        let mut argv = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        argv[s] = arg0;
        cb(cb_env, argv.as_ptr(), (1 + s) as i64)
    }
}

/// The shared core — key pair OWNED, callback box BORROWED, answer
/// +1-owned. Spec order: the callable gate fires before the lookup
/// (a present key does not excuse a non-callable callbackfn).
///
/// # Safety
/// `p` is null or a live Map; `cb_av` is borrowed bits alive across
/// the call.
pub(crate) unsafe fn map_get_or_insert_computed(
    p: *mut c_void,
    key_tag: i64,
    key_payload: i64,
    cb_av: u64,
) -> AnyValue {
    let release_key = || {
        if key_tag == torajs_rc::AnySlotTag::Heap as i64 && key_payload != 0 {
            unsafe { __torajs_value_drop_heap(key_payload as *mut c_void) };
        }
    };
    unsafe {
        let Some((cb_env, cb_entry)) = closure_boxed_entry(cb_av) else {
            release_key();
            __torajs_throw_type_error(c"callbackfn is not a function".as_ptr());
            return VALUE_UNDEFINED;
        };
        if p.is_null() {
            release_key();
            return VALUE_UNDEFINED;
        }
        let (mut found, mut vt, mut vp): (i64, i64, i64) = (0, 5, 0);
        __torajs_map_peek(p, key_tag, key_payload, &mut found, &mut vt, &mut vp);
        if found != 0 {
            release_key();
            // peek rc-bumped the hit for us — the box carries it out.
            return __torajs_anyv_box_from_pair(vt, vp);
        }
        // Miss — the callback sees the key; our owned stake keeps it
        // alive across the call, so the argv box is a borrow.
        let k_boxed = __torajs_anyv_box_from_pair(key_tag, key_payload);
        let r = call_cb1(cb_env, cb_entry, k_boxed);
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(r as *mut c_void);
            release_key();
            return VALUE_UNDEFINED;
        }
        // One stake for the map (set consumes it), and the callback's
        // own +1 rides out as the answer.
        let rt = __torajs_anyv_unbox_tag(r);
        let rp = __torajs_anyv_unbox_value(r);
        crate::payload_rc_inc(rt, rp);
        __torajs_map_set(p, key_tag, key_payload, rt, rp);
        r
    }
}

/// The typed-lane kernel — same contract as the core.
///
/// # Safety
/// As [`map_get_or_insert_computed`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_get_or_insert_computed(
    p: *mut c_void,
    key_tag: i64,
    key_payload: i64,
    cb_av: u64,
) -> u64 {
    unsafe { map_get_or_insert_computed(p, key_tag, key_payload, cb_av) }
}
