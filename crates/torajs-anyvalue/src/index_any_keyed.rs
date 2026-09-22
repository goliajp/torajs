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

use crate::nanbox::{
    AnyValue, as_double, as_int32, as_void_ptr, is_cell, is_double, is_int32, is_null, is_undefined,
};
use torajs_rc::Tag;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — pending-throw probe (non-consuming).
    fn __torajs_throw_check() -> i64;
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
            // Step 1 is ToPrimitive(key, string), and its answer can be
            // a Symbol — a Symbol wrapper's always is (§20.4.3.5), and
            // so is a `toString` that returns one. Step 2 keeps that
            // Symbol as the key; only a non-Symbol primitive goes on
            // to ToString. Skipping straight to ToString threw
            // "Cannot convert a symbol to a string" for `o[Object(s)]`.
            if crate::to_primitive::is_object_value(key) {
                let Some(prim) = crate::to_primitive::heap_to_primitive(ptr, true) else {
                    return core::ptr::null_mut();
                };
                // A hook that threw answers a placeholder with the
                // throw pending.
                if __torajs_throw_check() != 0 {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(prim);
                    return core::ptr::null_mut();
                }
                if is_cell(prim)
                    && (as_void_ptr(prim) as *const u8).add(4).cast::<u16>().read()
                        == Tag::Symbol as u16
                {
                    // `prim` is an owned share — it becomes the answer.
                    return as_void_ptr(prim);
                }
                let s = crate::nanbox_ffi::__torajs_anyv_to_str(prim) as *mut c_void;
                crate::nanbox_ffi::__torajs_anyv_rc_dec(prim);
                return s;
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
    unsafe { any_index_set_keyed_impl(recv, key, tag, value, recv_slot, true) };
}

/// §13.15.2 sloppy-assignment flavor of the keyed write — a [[Set]]
/// refusal answers `0` instead of recording the strict TypeError
/// (the `__torajs_any_index_set_soft` / `__torajs_any_member_set_soft`
/// twins, one key dispatch up).
///
/// # Safety
/// Same contract as [`__torajs_any_index_set_keyed`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_set_keyed_soft(
    recv: AnyValue,
    key: AnyValue,
    tag: u64,
    value: u64,
    recv_slot: *mut u64,
) -> i64 {
    unsafe { any_index_set_keyed_impl(recv, key, tag, value, recv_slot, false) }
}

unsafe fn any_index_set_keyed_impl(
    recv: AnyValue,
    key: AnyValue,
    tag: u64,
    value: u64,
    recv_slot: *mut u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        if is_int32(key) {
            return crate::index_any_set::any_index_set_impl(
                recv,
                as_int32(key) as i64,
                tag,
                value,
                recv_slot,
                throw_on_refusal,
            );
        }
        if is_double(key) {
            let d = as_double(key);
            if d.is_finite() && d.trunc() == d && d.abs() <= 9007199254740991.0 {
                return crate::index_any_set::any_index_set_impl(
                    recv,
                    d as i64,
                    tag,
                    value,
                    recv_slot,
                    throw_on_refusal,
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
                return crate::member_set::any_member_set_impl(
                    slot,
                    ptr as *mut c_void,
                    tag,
                    value,
                    -1,
                    throw_on_refusal,
                );
            }
        }
        // §6.2.5.6 PutValue — ToObject(base) before the deferred
        // ToPropertyKey, the read's mirror (bun runs the key's
        // toString first here; the spec and test262 do not).
        if is_null(recv) || is_undefined(recv) {
            __torajs_throw_type_error(c"cannot set properties of null or undefined".as_ptr());
            return 0;
        }
        let kcell = __torajs_anyv_to_property_key(key);
        if kcell.is_null() {
            return 0;
        }
        let wrote =
            crate::member_set::any_member_set_impl(slot, kcell, tag, value, -1, throw_on_refusal);
        __torajs_anyv_property_key_drop(kcell);
        wrote
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
        // A Proxy takes every key shape through the one [[Get]] —
        // the trap is entitled to the key the source wrote, coerced
        // to a property key (RFC 20260823-proxy-substrate 刀 1).
        if crate::proxy::is_proxy(recv) {
            return proxy_keyed_get(recv, key);
        }
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
        // §6.2.5.5 GetValue — ToObject(base) comes BEFORE the deferred
        // ToPropertyKey: `null[{toString}]` is the TypeError, and the
        // key's toString never runs (test262 compound-assignment
        // S11.13.2_A7.x T2; the cell / numeric arms above throw inside
        // their own kernels without coercing anything).
        if !require_object_coercible(recv) {
            return crate::nanbox::VALUE_UNDEFINED;
        }
        // §7.1.19 steps 1-3 — the key cell (a Symbol when the key's
        // ToPrimitive answers one), probe, release the temp.
        let kcell = __torajs_anyv_to_property_key(key);
        if kcell.is_null() {
            return crate::nanbox::VALUE_UNDEFINED;
        }
        let out = probe_key_cell(recv, kcell as *const c_void);
        __torajs_anyv_property_key_drop(kcell);
        out
    }
}

/// §7.1.18 RequireObjectCoercible on a receiver: records the read
/// TypeError and answers false on null / undefined, true otherwise.
/// The keyed read above asks it before its own ToString of the key;
/// the lowering asks it (through the extern twin) before the
/// up-front ToPropertyKey a compound / logical assignment runs so
/// its read and write share one coerced key — §6.2.5.5 GetValue
/// puts ToObject(base) first either way.
///
/// # Safety
/// `recv` is a valid AnyValue.
unsafe fn require_object_coercible(recv: AnyValue) -> bool {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr())
        };
        return false;
    }
    true
}

/// [`require_object_coercible`] for the lowering: a pending TypeError
/// on a nullish receiver, nothing otherwise; the emit site's throw
/// check propagates it.
///
/// # Safety
/// `recv` is a valid AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_require_object_coercible(recv: AnyValue) {
    unsafe {
        let _ = require_object_coercible(recv); // the verdict is the pending throw
    }
}

/// §7.1.19 ToPropertyKey then [[Get]], for a Proxy receiver. A
/// Symbol key passes through as itself; everything else takes its
/// string spelling, which is what the trap must see.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `key` a valid AnyValue.
unsafe fn proxy_keyed_get(recv: AnyValue, key: AnyValue) -> AnyValue {
    unsafe {
        if is_cell(key) {
            let ptr = as_void_ptr(key);
            let tag = (ptr as *const u8).add(4).cast::<u16>().read();
            if tag == Tag::Symbol as u16 || tag == Tag::Str as u16 {
                return crate::proxy::get_key_cell(recv, ptr as *const c_void);
            }
        }
        let s = __torajs_anyv_to_property_key(key);
        if s.is_null() {
            return crate::nanbox::VALUE_UNDEFINED;
        }
        let out = crate::proxy::get_key_cell(recv, s as *const c_void);
        __torajs_anyv_property_key_drop(s);
        out
    }
}
