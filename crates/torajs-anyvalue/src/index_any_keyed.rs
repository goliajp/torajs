//! `__torajs_any_index_get_keyed` — `recv[key]` where BOTH sides are
//! `any` values (cluster #1 blade 3; the all-any promoted-callback
//! body `obj[idx]` is the motivating shape).
//!
//! ES §7.1.19 ToPropertyKey dispatch on the runtime key tag:
//! - int32 / integral double → the numeric lane
//!   ([`crate::index_any::__torajs_any_index_get`]).
//! - Str cell / Symbol cell → the member probe pair uncoerced
//!   (§7.1.19 step 2 for symbols), accessor-aware (tag 6 = accessor
//!   entry; its getter runs via `__torajs_any_accessor_get`).
//! - everything else (short-str, bool, null, undefined, non-integral
//!   double, object cells) → `ToString(key)` per §7.1.19 step 3
//!   ([`crate::nanbox_ffi::__torajs_anyv_to_str`], spec §7.1.17,
//!   fresh owned Str released after the probe).
//!
//! Result carries the probe pair's owned (+1 for cells) convention —
//! the lowering records the read as owned, mirroring the str-key lane.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, as_double, as_int32, as_void_ptr, is_cell, is_double, is_int32};
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-str — release a heap Str reference (the temp ToString
    /// key minted below).
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-str — release a Symbol reference.
    fn __torajs_symbol_drop(s: *mut c_void);
    /// torajs-rc — take one more share of a heap cell.
    fn __torajs_rc_inc(p: *mut c_void);
}

/// §7.1.19 ToPropertyKey as a standalone answer: the key CELL a
/// runtime `any` names, owned by the caller.
///
/// The dispatch above resolves a key and probes in one breath, which
/// suits reads. The define family needs the key itself, to hand to a
/// kernel that stores it — and it had no way to ask, so it reached
/// for ToString and turned a symbol living in an `any` into the
/// string `"Symbol(x)"`: stored under the wrong key, and visible in
/// `Object.keys`, which no symbol ever is.
///
/// Step 2 hands a Symbol back untouched; everything else is step 3's
/// ToString. Either way the answer carries one share — release it
/// with [`__torajs_anyv_property_key_drop`], which is kind-aware
/// because the answer's kind is not knowable at the call site.
/// NULL means ToString threw and the caller must propagate.
///
/// # Safety
/// `key` is a valid AnyValue; a cell key points at a live heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_property_key(key: AnyValue) -> *mut c_void {
    unsafe {
        if is_cell(key) {
            let ptr = as_void_ptr(key);
            let tag = (ptr as *const u8).add(4).cast::<u16>().read();
            if tag == Tag::Symbol as u16 || tag == Tag::Str as u16 {
                __torajs_rc_inc(ptr);
                return ptr;
            }
        }
        crate::nanbox_ffi::__torajs_anyv_to_str(key) as *mut c_void
    }
}

/// Release what [`__torajs_anyv_to_property_key`] handed out.
///
/// # Safety
/// `p` is null or a share of a live Str / Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_property_key_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe {
        if (p as *const u8).add(4).cast::<u16>().read() == Tag::Symbol as u16 {
            __torajs_symbol_drop(p);
        } else {
            __torajs_str_drop(p);
        }
    }
}

const ANY_ACCESSOR_TAG: u64 = 6;

/// AnySlotTag heap-cell index in the probe pair's tag channel.
const ANY_HEAP_TAG: u64 = 4;

/// Member probe pair by a Str / Symbol cell key, accessor-aware.
///
/// The `member_get` pair is borrow-shaped on EVERY arm (its module
/// doc), while this entry's contract — the one all three consumers
/// (the keyed combinators' value collect, `settle_str_lane`'s
/// released read, the lowering's `owned_member_reads` bookkeeping)
/// release against — is owned. The +1 below is where the borrow is
/// promoted; without it a dict-held Promise collected by
/// `Promise.allKeyed` was released once more than it was retained
/// (rc underflow → pool recycle under a live `.then` receiver, the
/// rotation-348 crash family).
unsafe fn probe_key_cell(recv: AnyValue, key: *const c_void) -> AnyValue {
    unsafe {
        let tag = crate::member_get::__torajs_any_member_get_tag(recv, key);
        let value = crate::member_get::__torajs_any_member_get_value(recv, key);
        if tag == ANY_ACCESSOR_TAG {
            return crate::struct_probe::__torajs_any_accessor_get(recv, key, value);
        }
        if tag == ANY_HEAP_TAG && value != 0 {
            __torajs_rc_inc(value as *mut c_void);
        }
        crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64)
    }
}

/// Write mirror — `recv[key] = v` with an `any` key. Same §7.1.19
/// dispatch as the read: numeric keys ride
/// `__torajs_any_index_set`, Str / Symbol cells hand the key to the
/// member-set core uncoerced, everything else stores under its
/// ToString spelling. The (tag, value) pair transfers into the store
/// per the set cores' contract; a NULL `recv_slot` (temp receiver)
/// uses a local slot — relocation write-back is meaningless for a
/// temp, mirroring the numeric lane's NULL contract.
///
/// # Safety
/// Cell receivers / keys must be valid heap pointers; `recv_slot` is
/// NULL or points at a live AnyValue slot the caller owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_set_keyed(
    recv: AnyValue,
    key: AnyValue,
    tag: u64,
    value: u64,
    recv_slot: *mut u64,
) {
    unsafe {
        if is_int32(key) {
            return crate::index_any_set::__torajs_any_index_set(
                recv,
                as_int32(key) as i64,
                tag,
                value,
                recv_slot,
            );
        }
        if is_double(key) {
            let d = as_double(key);
            if d.is_finite() && d.trunc() == d && d.abs() <= 9007199254740991.0 {
                return crate::index_any_set::__torajs_any_index_set(
                    recv, d as i64, tag, value, recv_slot,
                );
            }
        }
        let mut local = recv;
        let slot: *mut AnyValue = if recv_slot.is_null() {
            &mut local
        } else {
            recv_slot as *mut AnyValue
        };
        if is_cell(key) {
            let ptr = as_void_ptr(key);
            let ktag = (ptr as *const u8).add(4).cast::<u16>().read();
            if ktag == Tag::Symbol as u16 || ktag == Tag::Str as u16 {
                return crate::member_set::__torajs_any_member_set(
                    slot,
                    ptr as *mut c_void,
                    tag,
                    value,
                    -1,
                );
            }
        }
        let kstr = crate::nanbox_ffi::__torajs_anyv_to_str(key);
        if kstr.is_null() {
            return;
        }
        crate::member_set::__torajs_any_member_set(slot, kstr, tag, value, -1);
        __torajs_str_drop(kstr);
    }
}

/// See module doc.
///
/// # Safety
/// Cell receivers / keys must be valid heap pointers matching their
/// header tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_get_keyed(recv: AnyValue, key: AnyValue) -> AnyValue {
    unsafe {
        if is_int32(key) {
            return crate::index_any::__torajs_any_index_get(recv, as_int32(key) as i64);
        }
        if is_double(key) {
            let d = as_double(key);
            if d.is_finite() && d.trunc() == d && d.abs() <= 9007199254740991.0 {
                return crate::index_any::__torajs_any_index_get(recv, d as i64);
            }
            // non-integral / non-finite → decimal ToString probe below
        }
        if is_cell(key) {
            let ptr = as_void_ptr(key);
            let tag = (ptr as *const u8).add(4).cast::<u16>().read();
            if tag == Tag::Symbol as u16 || tag == Tag::Str as u16 {
                return probe_key_cell(recv, ptr as *const c_void);
            }
            // other cells (objects, Substr views) fall to ToString
        }
        // §7.1.19 step 3 — ToString(key), probe, release the temp.
        // Symbols never reach here (cell arm above), so the §7.1.17
        // implicit-coercion TypeError face stays untouched.
        let kstr = crate::nanbox_ffi::__torajs_anyv_to_str(key);
        if kstr.is_null() {
            return crate::nanbox::VALUE_UNDEFINED;
        }
        let out = probe_key_cell(recv, kstr as *const c_void);
        __torajs_str_drop(kstr);
        out
    }
}
