//! Cached immortal Str keys for the exec-shape match-result props
//! (`index` / `input` / `groups` / `indices`) — pulled out of
//! `match_op.rs` when the `/d` match-indices face landed (file-size
//! decomp; the caching machinery is its own concern).

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use super::{__torajs_str_drop, str_from_bytes};

/// `torajs_rc::FLAG_STATIC_LITERAL` — `1 << 2 = 4`. Mirrored as a
/// local const so torajs-regex doesn't need a torajs-rc cargo dep
/// for one flag bit. When set on a `HeapHeader.flags` field (offset
/// 6 on the universal layout), `__torajs_rc_inc` / `__torajs_rc_dec`
/// / `__torajs_str_drop` all no-op — the heap block is immortal
/// for the program's lifetime. Unit test
/// `torajs_rc::tests::flag_static_literal_value_locked` (lib.rs:657)
/// asserts the value stays `4`.
const FLAG_STATIC_LITERAL: u16 = 1 << 2;

/// Round 4 wire-back Phase B chunk 1 attacks #R-K1 + #R-K3 — cached
/// `"index"` / `"input"` / `"groups"` key Str slots. First call into
/// each `attach_*` allocates the Str via `str_from_bytes`, stamps
/// `FLAG_STATIC_LITERAL` on the header, and CASes the result into
/// the static slot; subsequent calls fast-path the atomic-load
/// (~1 ns vs ~32 ns alloc). `__torajs_str_drop` after `arrprops_set`
/// also no-ops on the flag, saving an additional ~3 ns per key.
/// Per-call gain: ~35 ns/key × 3 keys = ~105 ns/iter on the
/// `regex-wireback-minlit-100k` fixture; aligns the decomp
/// (`.claude/rfcs/20260625-perf-wire-back-decomp/decomposition.md`)
/// Top-N estimate.
///
/// **Race semantics**: under v0.2 single-mutator the CAS always
/// succeeds first time; once the multi-threaded substrate lands
/// (v1.0 biased-ARC, design-principles.md §6.2), a losing-CAS
/// thread cleans up its fresh alloc and adopts the winner. The
/// only observable cost of a race loss is the cleanup `str_drop`
/// path (~30 ns); bounded by thread count, occurs once per slot.
pub(super) static K_INDEX: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
pub(super) static K_INPUT: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
pub(super) static K_GROUPS: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// `"indices"` — the §22.2.7.8 MakeIndicesArray own prop attached
/// by [`super::match_indices::attach_indices`] on `/d` regexes.
pub(super) static K_INDICES: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// Returns the cached immortal Str for `bytes`, allocating on first
/// call. Subsequent calls hit the atomic-load fast path. See
/// [`K_INDEX`] doc for race semantics + perf framing.
///
/// # Safety
///
/// `bytes` must be a stable literal (the first call's content seeds
/// the cache forever); all wireback call sites pass `b"..."` byte
/// literals.
pub(super) unsafe fn cached_static_key(slot: &AtomicPtr<c_void>, bytes: &[u8]) -> *const c_void {
    let cached = slot.load(Ordering::Relaxed);
    if !cached.is_null() {
        return cached as *const c_void;
    }
    let fresh = unsafe { str_from_bytes(bytes) } as *mut c_void;
    // Stamp FLAG_STATIC_LITERAL so rc_inc / rc_dec / str_drop all
    // no-op on this header — the slot becomes immortal. HeapHeader
    // layout: refcount u32 @0, type_tag u16 @4, flags u16 @6
    // (see torajs_rc::lib.rs `pub struct HeapHeader`).
    unsafe {
        let flags_ptr = (fresh as *mut u8).add(6) as *mut u16;
        *flags_ptr |= FLAG_STATIC_LITERAL;
    }
    match slot.compare_exchange(
        core::ptr::null_mut(),
        fresh,
        Ordering::AcqRel,
        Ordering::Relaxed,
    ) {
        Ok(_) => fresh as *const c_void,
        Err(other) => {
            // CAS race lost — clear our immortal flag so str_drop
            // can genuinely free the fresh alloc (otherwise the
            // STATIC_LITERAL gate would short-circuit drop and leak
            // ~16 bytes per losing race).
            unsafe {
                let flags_ptr = (fresh as *mut u8).add(6) as *mut u16;
                *flags_ptr &= !FLAG_STATIC_LITERAL;
                __torajs_str_drop(fresh as *mut c_void);
            }
            other as *const c_void
        }
    }
}
