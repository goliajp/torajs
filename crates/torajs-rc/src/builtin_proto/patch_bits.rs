//! Per-tag method-id bitmasks for the builtin prototypes — which
//! method ids a program has DELETED off one, and which it has
//! OVERWRITTEN on one.
//!
//! Split from [`super`], which owns the prototype singletons
//! themselves. Two identities that only look alike: the parent
//! answers "give me %Array.prototype%", this one answers "has
//! anything been done to slot N of it". The masks are owner-side
//! flags because the interned method cells are immortal — `delete
//! String.prototype.small` cannot remove one, so the deleted state
//! has to live beside the owner rather than in the cell.
//!
//! Every symbol here is `no_mangle`, so moving them changes no ABI.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use super::{__torajs_builtin_proto_tag_of, NUM_BUILTIN_PROTOS};

/// Method-id span of the per-tag deleted bitmask below — mirrors
/// `torajs-anyvalue::method_value::TABLE_SIZE` (the intern-table
/// span; ids are append-only).
const DELETED_MASK_WORDS: usize = 4; // 256 mids / 64 bits

// Per-tag deleted-method-id bitmask (RFC 20260712 chunk 3) — the
// `FLAG_FN_NAME_DELETED` precedent generalized: `delete
// String.prototype.small` cannot remove the immortal interned method
// cell, so the deleted state lives here as an owner-side flag. A
// dynobj own entry (monkey-patch / defineProperty restore) always
// wins before any intern fallthrough consults this mask, so a re-set
// revives without a clear call. AtomicU64 fetch_or / load per
// design-principles §6.2 (multi-thread-ready shape).
#[allow(clippy::declare_interior_mutable_const)]
const MASK_INIT: [AtomicU64; DELETED_MASK_WORDS] =
    [const { AtomicU64::new(0) }; DELETED_MASK_WORDS];
static DELETED_MIDS: [[AtomicU64; DELETED_MASK_WORDS]; NUM_BUILTIN_PROTOS] =
    [MASK_INIT; NUM_BUILTIN_PROTOS];

/// Mark `<proto tag>`'s interned method `mid` as deleted. Idempotent;
/// out-of-range inputs are ignored (defensive — callers gate on the
/// family-owns check first).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_mark_deleted(tag: i64, mid: i64) {
    let (t, m) = (tag as usize, mid as usize);
    if t >= NUM_BUILTIN_PROTOS || m >= DELETED_MASK_WORDS * 64 {
        return;
    }
    DELETED_MIDS[t][m / 64].fetch_or(1u64 << (m % 64), Ordering::AcqRel);
}

/// 1 = `<proto tag>`'s interned method `mid` was deleted (and no
/// dynobj own entry has since shadowed the question — the caller
/// probes entries first). Out-of-range inputs answer 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_is_deleted(tag: i64, mid: i64) -> i64 {
    let (t, m) = (tag as usize, mid as usize);
    if t >= NUM_BUILTIN_PROTOS || m >= DELETED_MASK_WORDS * 64 {
        return 0;
    }
    ((DELETED_MIDS[t][m / 64].load(Ordering::Acquire) >> (m % 64)) & 1) as i64
}

// "Some `<Ctor>.prototype` singleton exists" front gate for the
// own-write note — set once at mint time, read on every dynobj own
// write (RFC 20260721 刀 11 G13).
pub(super) static ANY_MINTED: AtomicU64 = AtomicU64::new(0);

// Per-tag patched-method-id bitmask (RFC 20260721 刀 11 G13) — the
// DELETED_MIDS shape mirrored for the opposite question: a user own
// entry written onto a builtin prototype singleton under an interned
// method name (monkey-patch / defineProperty, data or accessor) sets
// its (tag, mid) bit, and the primitive fast arms in the any-method
// dispatcher consult it BEFORE answering natively. Sticky: an entry
// delete leaves the bit set — the consult's own probe then misses
// and the fast arm keeps winning, so a stale bit costs one probe,
// never a wrong answer.
#[allow(clippy::declare_interior_mutable_const)]
const PATCH_MASK_INIT: [AtomicU64; DELETED_MASK_WORDS] =
    [const { AtomicU64::new(0) }; DELETED_MASK_WORDS];
static PATCHED_MIDS: [[AtomicU64; DELETED_MASK_WORDS]; NUM_BUILTIN_PROTOS] =
    [PATCH_MASK_INIT; NUM_BUILTIN_PROTOS];

/// Own-entry write note — every kernel that can create an own entry
/// on a dynobj calls here with the target and the key's name bytes.
/// `obj` values that are not a minted builtin-prototype singleton
/// (the overwhelmingly common case) return after one relaxed load /
/// one O(14) address scan; a singleton hit interns the name and
/// marks the (tag, mid) patch bit.
///
/// # Safety
/// `obj` is any pointer (compared, never dereferenced); `name` /
/// `len` describe live UTF-8 key bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_note_own_write(
    obj: *const c_void,
    name: *const u8,
    len: i64,
) {
    if ANY_MINTED.load(Ordering::Acquire) == 0 {
        return;
    }
    let tag = unsafe { __torajs_builtin_proto_tag_of(obj) };
    if tag < 0 || name.is_null() || len <= 0 {
        return;
    }
    let bytes = unsafe { core::slice::from_raw_parts(name, len as usize) };
    let Ok(s) = core::str::from_utf8(bytes) else {
        return;
    };
    let mid = crate::any_method_id(s);
    let (t, m) = (tag as usize, mid as usize);
    if mid == crate::ANY_METHOD_UNKNOWN || m >= DELETED_MASK_WORDS * 64 {
        return;
    }
    PATCHED_MIDS[t][m / 64].fetch_or(1u64 << (m % 64), Ordering::AcqRel);
}

/// 1 = a user own entry was written on `<proto tag>`'s singleton
/// under interned method `mid` at some point (see the sticky note
/// above). The primitive fast-arm pre-gate's single load.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_has_patch(tag: i64, mid: i64) -> i64 {
    let (t, m) = (tag as usize, mid as usize);
    if t >= NUM_BUILTIN_PROTOS || m >= DELETED_MASK_WORDS * 64 {
        return 0;
    }
    ((PATCHED_MIDS[t][m / 64].load(Ordering::Acquire) >> (m % 64)) & 1) as i64
}

/// 1 = SOMETHING happened to `<proto tag>`'s method `mid` that the
/// native arms must not answer over — a user own entry was written
/// (`has_patch`) or the method was deleted (`is_deleted`).
///
/// The two are one question for a dispatcher: either way the builtin
/// surface is no longer what a call resolves to, and the caller has
/// to take the slow path to find out which. Keeping them as one
/// front gate is what lets the unpatched, undeleted program pay two
/// relaxed loads and nothing else per method call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_is_shadowed(tag: i64, mid: i64) -> i64 {
    let (t, m) = (tag as usize, mid as usize);
    if t >= NUM_BUILTIN_PROTOS || m >= DELETED_MASK_WORDS * 64 {
        return 0;
    }
    let bit = 1u64 << (m % 64);
    let patched = PATCHED_MIDS[t][m / 64].load(Ordering::Acquire) & bit;
    let deleted = DELETED_MIDS[t][m / 64].load(Ordering::Acquire) & bit;
    ((patched | deleted) != 0) as i64
}
