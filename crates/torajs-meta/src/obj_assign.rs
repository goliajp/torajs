//! `Object.assign(target, source)` for an `any`-typed target —
//! ES §20.1.2.1 runtime walk (the static struct-target lane lives in
//! ssa_lower_call_object_assign; this kernel is its `any` sibling,
//! same family as the define_all / own_keys runtime walks).
//!
//! Per §20.1.2.1: a nullish TARGET throws (step 1 ToObject); a
//! nullish SOURCE contributes nothing (step 4.a); each own
//! enumerable string key of the source is read with [[Get]] (through
//! a getter when the probe answers the accessor sentinel) and written
//! to the target with [[Set]] (through setters / the wrapper and
//! non-extensible gates member_set already carries). A getter or
//! setter throw ends the walk — the pending throw propagates through
//! the caller's throw check.
//!
//! Ownership: `member_get_value` answers borrow-shaped bits, so the
//! heap case takes +1 before feeding member_set's consume contract;
//! `any_accessor_get` answers an OWNED value whose stake transfers
//! straight through. The keys array is this fn's own mint — dropped
//! on every exit.

use core::ffi::c_void;

use crate::obj_own_keys::{ANY_HEAP_TAG, ARR_LEN_OFF};
use crate::reflect::{VALUE_NULL_IMM, VALUE_UNDEFINED_IMM};

/// `struct_probe::ANY_ACCESSOR_TAG` mirror — the member-get tag
/// channel's accessor sentinel.
const ACCESSOR_TAG: u64 = 6;

/// torajs-arr cell layout mirror (`layout.rs` B1 fixed cell) — same
/// cross-crate sync posture as obj_own_values' walks.
const ARR_DATA_PTR_OFF: usize = 32;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_any_member_get_tag(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_member_get_value(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_accessor_get(recv: u64, key: *const c_void, pair_bits: u64) -> u64;
    fn __torajs_any_member_set(
        recv_slot: *mut u64,
        key: *mut c_void,
        tag: u64,
        value: u64,
        hint: i64,
    );
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Copy `source`'s own enumerable string-keyed properties into
/// `target` per §20.1.2.1 step 4.c. One source per call — the
/// lowering loops left-to-right so last-source-wins falls out of
/// write order.
///
/// # Safety
/// `target` / `source` are live AnyValue bit patterns the caller
/// owns; the caller must check for a pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_assign(target: u64, source: u64) {
    if target == VALUE_NULL_IMM || target == VALUE_UNDEFINED_IMM {
        unsafe {
            __torajs_throw_type_error(c"Cannot convert undefined or null to object".as_ptr());
        }
        return;
    }
    let mut t_slot = target;
    unsafe { copy_own_into(&mut t_slot, source) };
}

/// `{ ...source }` into the dynobj lane's fresh literal — §7.3.25
/// CopyDataProperties (rotation 267). Same own-enumerable walk as
/// [`__torajs_anyv_assign`]: a fresh plain dynobj has no setters or
/// prototype, so the walk's [[Set]] ≡ CreateDataProperty. `obj_slot`
/// is the lane's live dynobj POINTER slot — member_set may resize
/// the block, and the fresh pointer rides the anyv slot back out.
///
/// # Safety
/// `obj_slot` points at a live dynobj pointer; `source` is a live
/// AnyValue bit pattern. Caller must check for a pending throw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_spread_from(obj_slot: *mut *mut c_void, source: u64) {
    let mut t_anyv =
        unsafe { __torajs_anyv_box_from_pair(ANY_HEAP_TAG as i64, (*obj_slot) as i64) };
    unsafe { copy_own_into(&mut t_anyv, source) };
    unsafe { *obj_slot = __torajs_anyv_unbox_value(t_anyv) as *mut c_void };
}

/// Shared own-enumerable copy — §20.1.2.1 step 4 / §7.3.25 body. A
/// nullish source contributes nothing (step 4.a / step 1). Own
/// enumerable keys — full ToObject taxonomy (dynobj / struct / arr /
/// str / closure / wrapper arms) lives in the keys kernel; this walk
/// is shape-blind. OwnPropertyKeys includes the §10.1.11.1 SYMBOL
/// bucket (enumerable-filtered, same as the string buckets) — a
/// second pass over the same per-key body, because the two buckets
/// come from separate kernels by construction.
unsafe fn copy_own_into(target_slot: *mut u64, source: u64) {
    if source == VALUE_NULL_IMM || source == VALUE_UNDEFINED_IMM {
        return;
    }
    let strings = unsafe { crate::obj_own_keys::__torajs_anyv_own_keys(source, 0) };
    let threw = unsafe { copy_keys(target_slot, source, strings) };
    if !threw {
        let symbols = unsafe { crate::own_names::__torajs_anyv_own_enum_symbols(source) };
        unsafe { copy_keys(target_slot, source, symbols) };
    }
}

/// Copy every key in a keys array from `source` into the target
/// anyv slot, then release the array and the key stakes it holds.
/// Returns `true` when a getter or setter recorded a pending throw,
/// which ends the assign. The slot form keeps a dynobj target valid
/// across a member_set resize (the kernel writes the relocated box
/// back through the slot; the old per-key `let mut t_slot = target`
/// re-read a stale box after a resize).
///
/// # Safety
/// `keys` is an owned `+1`-rc keys array this call consumes;
/// `target_slot` points at a live AnyValue; `source` is a live
/// AnyValue bit pattern.
unsafe fn copy_keys(target_slot: *mut u64, source: u64, keys: *mut c_void) -> bool {
    let len = unsafe { (keys.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let data = unsafe { (keys.cast::<u8>().add(ARR_DATA_PTR_OFF) as *const *const u64).read() };
    let mut threw = false;
    for i in 0..len as usize {
        let key = unsafe { data.add(i).read() } as *mut c_void;
        if key.is_null() {
            continue;
        }
        let tag = unsafe { __torajs_any_member_get_tag(source, key) };
        let (vtag, vval) = if tag == ACCESSOR_TAG {
            // [[Get]] through the getter — owned result; a throwing
            // getter records the pending throw checked below.
            let pair_bits = unsafe { __torajs_any_member_get_value(source, key) };
            let owned = unsafe { __torajs_any_accessor_get(source, key, pair_bits) };
            if unsafe { __torajs_throw_check() } != 0 {
                threw = true;
                break;
            }
            let t = unsafe { __torajs_anyv_unbox_tag(owned) } as u64;
            let v = unsafe { __torajs_anyv_unbox_value(owned) } as u64;
            (t, v)
        } else {
            // Borrow-shaped probe answer — retain before feeding
            // member_set's consume contract.
            let v = unsafe { __torajs_any_member_get_value(source, key) };
            if tag == ANY_HEAP_TAG as u64 && v != 0 {
                unsafe { __torajs_rc_inc(v as *mut c_void) };
            }
            (tag, v)
        };
        unsafe { __torajs_any_member_set(target_slot, key, vtag, vval, -1) };
        if unsafe { __torajs_throw_check() } != 0 {
            threw = true;
            break;
        }
    }
    // The keys walk retained (or minted) every key it pushed, but the
    // array's elem kind is UNSET — its drop won't cascade. Release
    // each key stake by hand (including unvisited ones after an early
    // break), then the skeleton.
    for i in 0..len as usize {
        let key = unsafe { data.add(i).read() } as *mut c_void;
        if !key.is_null() {
            unsafe { __torajs_value_drop_heap(key) };
        }
    }
    unsafe { __torajs_value_drop_heap(keys as *mut c_void) };
    threw
}
