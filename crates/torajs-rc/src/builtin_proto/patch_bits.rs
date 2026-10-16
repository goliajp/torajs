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
use crate::str_wtf8::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_FLAGS_SUBSTR, STR_LEN_OFF};

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
// design principles §6.2 (multi-thread-ready shape).
#[allow(clippy::declare_interior_mutable_const)]
const MASK_INIT: [AtomicU64; DELETED_MASK_WORDS] =
    [const { AtomicU64::new(0) }; DELETED_MASK_WORDS];
static DELETED_MIDS: [[AtomicU64; DELETED_MASK_WORDS]; NUM_BUILTIN_PROTOS] =
    [MASK_INIT; NUM_BUILTIN_PROTOS];

/// Mark `<proto tag>`'s interned method `mid` as deleted. Idempotent;
/// out-of-range inputs are ignored (defensive — callers gate on the
/// family-owns check first).
///
/// # Safety
///
/// Any `tag` / `mid` pair is accepted; out-of-range inputs are ignored
/// or answer 0. The masks are process-global atomics.
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
///
/// # Safety
///
/// Any `tag` / `mid` pair is accepted; out-of-range inputs are ignored
/// or answer 0. The masks are process-global atomics.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_is_deleted(tag: i64, mid: i64) -> i64 {
    let (t, m) = (tag as usize, mid as usize);
    if t >= NUM_BUILTIN_PROTOS || m >= DELETED_MASK_WORDS * 64 {
        return 0;
    }
    ((DELETED_MIDS[t][m / 64].load(Ordering::Acquire) >> (m % 64)) & 1) as i64
}

// Front gate for the own-write note: one bit per address hash of
// every minted `<Ctor>.prototype` singleton, set before the singleton
// is published. A clear bit proves the target is no singleton, so an
// ordinary own write pays one load and a bit test; a set bit (a
// singleton, or another address sharing its hash) falls to the scan.
// A single "anything minted" flag sent every own write of a program
// that once read a missing key, which mints %Object.prototype%, into
// the scan.
const MINT_FILTER_WORDS: usize = 16;
#[allow(clippy::declare_interior_mutable_const)]
const MINT_FILTER_INIT: AtomicU64 = AtomicU64::new(0);
static MINT_FILTER: [AtomicU64; MINT_FILTER_WORDS] = [MINT_FILTER_INIT; MINT_FILTER_WORDS];

/// The filter bit of `addr`: bits 4..14 of the cell address (cells are
/// 16-byte aligned) pick one of the filter's 1024 bits — a word and a
/// shift, two bitfield extracts with no constant to load.
#[inline]
fn mint_filter_bit(addr: usize) -> (usize, u32) {
    let h = (addr >> 4) & 1023;
    (h >> 6, (h & 63) as u32)
}

/// Record a singleton about to be published (the parent's mint).
pub(super) fn note_minted(addr: usize) {
    let (w, b) = mint_filter_bit(addr);
    MINT_FILTER[w].fetch_or(1 << b, Ordering::Release);
}

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

/// Own-entry change note — every kernel that writes, defines or
/// deletes an own entry calls here with the target and the key cell
/// (null for a delete). `obj` values that are not a minted
/// builtin-prototype singleton (the overwhelmingly common case)
/// return after one filter-bit test, rarely an address scan, so the
/// call sites stay a bare call; a singleton hit raises its
/// [`TOUCHED`] bit and, for a key that spells an interned method
/// name, marks the (tag, mid) patch bit.
///
/// # Safety
/// `obj` is any pointer (compared, never dereferenced); `key` is null
/// or a live Str or Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_note_key(obj: *const c_void, key: *const c_void) {
    let (w, b) = mint_filter_bit(obj as usize);
    if (MINT_FILTER[w].load(Ordering::Acquire) >> b) & 1 == 0 {
        return;
    }
    unsafe { note_singleton_write(obj, key) }
}

/// The note's rare half, out of line so the common return above needs
/// no stack frame.
///
/// # Safety
/// As [`__torajs_builtin_proto_note_key`].
#[cold]
#[inline(never)]
unsafe fn note_singleton_write(obj: *const c_void, key: *const c_void) {
    let tag = unsafe { __torajs_builtin_proto_tag_of(obj) };
    if tag < 0 {
        return;
    }
    TOUCHED.fetch_or(1 << tag, Ordering::AcqRel);
    // only a Str cell owning a Latin-1 payload can spell a method name;
    // a Symbol keeps a pointer where a Str keeps its length
    if key.is_null() {
        return;
    }
    let k = key.cast::<u8>();
    let flags = unsafe { k.add(6).cast::<u16>().read() };
    if unsafe { k.add(4).cast::<u16>().read() } != crate::Tag::Str as u16
        || flags & (STR_FLAG_IS_LATIN1 | STR_FLAGS_SUBSTR) != STR_FLAG_IS_LATIN1
    {
        return;
    }
    let len = unsafe { k.add(STR_LEN_OFF).cast::<u32>().read() } as usize;
    let bytes = unsafe { core::slice::from_raw_parts(k.add(STR_DATA_OFF), len) };
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
///
/// # Safety
///
/// Any `tag` / `mid` pair is accepted; out-of-range inputs are ignored
/// or answer 0. The masks are process-global atomics.
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
///
/// # Safety
///
/// Any `tag` / `mid` pair is accepted; out-of-range inputs are ignored
/// or answer 0. The masks are process-global atomics.
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

// One bit per prototype tag, raised the first time an own entry of
// that singleton is created, overwritten or redefined after it is
// published (its [[Prototype]] link is such an entry), or a delete
// reaches it — the dynobj delete notes before its probe, so deleting
// an interned method, which has no entry, raises it too. Never
// cleared. A reader asking
// "does the singleton still hold what its mint installed" cannot
// answer from the entry counts: a full dense array compacts its holes
// away, so deletes followed by adds can bring both counts back to the
// minted values over a changed key set. Integrity-level changes
// (seal / freeze / preventExtensions) do not raise it: they change no
// value a read of the singleton answers.
static TOUCHED: AtomicU64 = AtomicU64::new(0);
const _: () = assert!(NUM_BUILTIN_PROTOS <= 64);

/// 1 = an own entry of `<proto tag>`'s singleton changed after its
/// mint (see [`TOUCHED`]); 0 for a singleton never minted. `tag` is a
/// prototype tag, below [`NUM_BUILTIN_PROTOS`].
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_singleton_touched(tag: i64) -> i64 {
    // no data is published with the bit, so no ordering is owed
    ((TOUCHED.load(Ordering::Relaxed) >> (tag as u32 & 63)) & 1) as i64
}
