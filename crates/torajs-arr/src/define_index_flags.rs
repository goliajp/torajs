//! The array element domain's per-index ATTRIBUTES — the
//! `defineProperty` shadow entries and the cell's integrity level,
//! read back as one flags word.
//!
//! Split out of `define.rs` under the 500-line file rule when the
//! §7.3.15 integrity fold landed; `define.rs` owns the write side of
//! the same shadows.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_EXOTIC_INDEX, FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED};

use crate::define::{
    F_CONFIGURABLE, F_HOLE, F_WRITABLE, FLAGS_DEFAULT, header_flags, mint_index_key, props_slot,
};

unsafe extern "C" {
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_flags(dynobj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// §7.3.15 SetIntegrityLevel over the ELEMENT domain — mask an
/// index's attributes down to the level the header records. `freeze`
/// clears writable + configurable on every own property, `seal`
/// clears configurable; both are header bits (`__torajs_obj_freeze`
/// sets them), and the element domain has no per-index storage to
/// stamp, so the level folds in here instead. A hole owns nothing and
/// passes through.
///
/// Pre-fix `Object.freeze(arr)` marked the header and stopped: the
/// dynobj entry walk it pairs with has no elements to visit, so
/// `getOwnPropertyDescriptor` still reported writable / configurable
/// true, `delete arr[i]` succeeded, and `arr[i] = v` MUTATED a frozen
/// array while `Object.isFrozen` answered true.
#[inline]
fn apply_integrity_level(flags: u64, hflags: u16) -> u64 {
    if flags & F_HOLE != 0 {
        return flags;
    }
    if hflags & FLAG_FROZEN != 0 {
        return flags & !(F_WRITABLE | F_CONFIGURABLE);
    }
    if hflags & FLAG_SEALED != 0 {
        return flags & !F_CONFIGURABLE;
    }
    flags
}

/// Current attribute flags of index `idx` — the shadow entry when one
/// exists, the implicit defaults otherwise, in both cases masked by
/// the cell's integrity level. `key` is the caller's index Str
/// (avoids a re-mint).
pub(crate) unsafe fn index_flags_with_key(arr: *const c_void, key: *const c_void) -> u64 {
    let hflags = unsafe { header_flags(arr) };
    if hflags & FLAG_ARR_EXOTIC_INDEX == 0 {
        return apply_integrity_level(FLAGS_DEFAULT, hflags);
    }
    let props = unsafe { *props_slot(arr as *mut c_void) };
    if props.is_null() || unsafe { __torajs_dynobj_has(props, key) } == 0 {
        return apply_integrity_level(FLAGS_DEFAULT, hflags);
    }
    if unsafe { __torajs_dynobj_entry_is_hole(props, key) } != 0 {
        return F_HOLE;
    }
    apply_integrity_level(unsafe { __torajs_dynobj_get_flags(props, key) }, hflags)
}

/// `Object.getOwnPropertyDescriptor` / element-write flags probe —
/// mint the canonical index key, read the shadow entry (or defaults).
/// Fast path: exotic bit clear → defaults with zero allocation.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64 {
    let hflags = unsafe { header_flags(arr) };
    // Sparse tail (RFC 20260810) — an index over the materialized
    // extent is an implicit hole: absent, no shadow entry to probe.
    if hflags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0
        && idx >= unsafe { crate::layout::arr_live_extent(arr as *const u8) }
    {
        return F_HOLE;
    }
    if hflags & FLAG_ARR_EXOTIC_INDEX == 0 {
        return apply_integrity_level(FLAGS_DEFAULT, hflags);
    }
    let key = unsafe { mint_index_key(idx) };
    let flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
    unsafe { __torajs_str_drop(key as *mut c_void) };
    flags
}

/// §10.4.2.1 — whether an index store has to be refused. A live own
/// index refuses when it is not writable; a HOLE is ABSENT, so the
/// store CREATES the property and only a non-extensible cell refuses
/// it (`Object.freeze` implies non-extensible, so a frozen array's
/// hole stays a hole). An out-of-range index owns nothing and is left
/// to the callers' own OOB handling.
///
/// Both element-store lanes ask this: the `any`-receiver kernel and
/// the typed tier's emitted pre-store gate. Callers screen on the
/// header bits first, so a plain array never reaches here.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_refuses_store(arr: *const c_void, idx: u64) -> i32 {
    let hflags = unsafe { header_flags(arr) };
    let flags = unsafe { __torajs_arr_index_flags(arr, idx) };
    let refused = if flags & F_HOLE != 0 {
        hflags & FLAG_NON_EXTENSIBLE != 0
    } else {
        flags & F_WRITABLE == 0
    };
    refused as i32
}

/// Typed-tier twin of [`__torajs_arr_index_refuses_store`] shaped
/// like `__torajs_obj_check_not_frozen`: arms the catchable TypeError
/// instead of answering, so the emitted gate is one call plus the
/// lowering's usual throw-check. The emitted code screens the header
/// bits first, so a plain array never calls this.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_check_store(arr: *const c_void, idx: u64) {
    if unsafe { __torajs_arr_index_refuses_store(arr, idx) } != 0 {
        unsafe {
            __torajs_throw_type_error(c"cannot assign to a read-only property".as_ptr());
        }
    }
}
