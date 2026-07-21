//! `Function.prototype`'s virtual own `name` / `length` pair (RFC
//! 20260722-builtin-proto-reflection 刀 3) — §20.2.3: the
//! %Function.prototype% intrinsic is itself callable, so it owns
//! the fn meta pair as data properties (`name` "", `length` 0),
//! both `{ writable: false, enumerable: false, configurable:
//! true }`. Split from `method_support_proto.rs` (500-line limit).
//!
//! Tombstone posture mirrors the virtual `constructor` face: the
//! non-interning ANY_METHOD_FN_PROTO_{NAME,LENGTH}_SLOT pseudo-ids
//! index the per-tag deleted bitmask; a defineProperty recreate
//! lands in the singleton's own dynobj entries, which every reader
//! probes first.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_FN_PROTO_LENGTH_SLOT, ANY_METHOD_FN_PROTO_NAME_SLOT};

/// `Function.prototype`'s builtin-proto tag (`torajs-rc
/// builtin_proto` order).
const FUNCTION_PROTO_TAG: i64 = 13;

/// Interned immortal `""` Str cell — the `name` value (lazy mint,
/// same shape as the ctor `.name` intern).
static EMPTY_NAME_CELL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn empty_name_cell() -> *mut u8 {
    let p = EMPTY_NAME_CELL.load(core::sync::atomic::Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = crate::method_value::mint_immortal_str(b"");
    EMPTY_NAME_CELL.store(cell as u64, core::sync::atomic::Ordering::Relaxed);
    cell
}

/// The virtual meta pair a builtin proto singleton owns under `key`
/// — member-pair convention: `(4, str cell)` for `name`, `(2, 0)`
/// for `length`. `None` off `Function.prototype`, on a
/// non-meta key, or behind a delete tombstone.
///
/// # Safety
/// `ptr` is NULL or a live heap cell; `key` is NULL or a live Str
/// cell.
pub(crate) unsafe fn builtin_proto_own_meta(
    ptr: *const c_void,
    key: *const c_void,
) -> Option<(u64, u64)> {
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    if tag != FUNCTION_PROTO_TAG {
        return None;
    }
    let slot = if unsafe { crate::prop_has::key_is(key, b"name") } {
        ANY_METHOD_FN_PROTO_NAME_SLOT
    } else if unsafe { crate::prop_has::key_is(key, b"length") } {
        ANY_METHOD_FN_PROTO_LENGTH_SLOT
    } else {
        return None;
    };
    if unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, slot) } != 0 {
        return None;
    }
    Some(if slot == ANY_METHOD_FN_PROTO_NAME_SLOT {
        (4, empty_name_cell() as u64)
    } else {
        (2, 0)
    })
}

/// gOPD's extern face — 1 on a hit with the pair written through
/// the out params; torajs-meta assembles the `{ writable: false,
/// enumerable: false, configurable: true }` data descriptor.
///
/// # Safety
/// Same contract as [`builtin_proto_own_meta`]; `out_tag` /
/// `out_val` are valid writable slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_meta(
    ptr: *const c_void,
    key: *const c_void,
    out_tag: *mut u64,
    out_val: *mut u64,
) -> i64 {
    match unsafe { builtin_proto_own_meta(ptr, key) } {
        Some((t, v)) => {
            unsafe {
                *out_tag = t;
                *out_val = v;
            }
            1
        }
        None => 0,
    }
}

/// The tombstone pseudo-slot `key` names on `Function.prototype` —
/// the delete hook's resolver. `None` off the fn proto / non-meta
/// keys.
///
/// # Safety
/// Same contract as [`builtin_proto_own_meta`].
pub(crate) unsafe fn fn_proto_meta_slot(ptr: *const c_void, key: *const c_void) -> Option<i64> {
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    if tag != FUNCTION_PROTO_TAG {
        return None;
    }
    if unsafe { crate::prop_has::key_is(key, b"name") } {
        Some(ANY_METHOD_FN_PROTO_NAME_SLOT)
    } else if unsafe { crate::prop_has::key_is(key, b"length") } {
        Some(ANY_METHOD_FN_PROTO_LENGTH_SLOT)
    } else {
        None
    }
}
