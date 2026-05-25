//! Shared observer registry — `target → list of (kind, owner) observers`.
//!
//! Port of `runtime_weakref.c`'s registry section (P4.3'-b, 2026-05-24).
//! The registry is **process-global** by design: every `WeakRef` /
//! `WeakMap` / `WeakSet` keys observers by their target pointer so a
//! single dying-target broadcast walks every observer kind in one pass.
//!
//! ## Data shape
//!
//! ```text
//!  G_BUCKETS[1024] ─┬──> TargetCell { target, observers ──┐ next }
//!                   │       ▲ (hash chain)                │
//!                   │       │                             │
//!                   │     TargetCell { target,            │
//!                   │       observers ──> ObserverNode {  │
//!                   │           kind, owner,              │
//!                   │           next ──> ObserverNode {   │
//!                   │               kind, owner,          │
//!                   │               next = NULL  }}}      │
//!                   │                                     │
//!                   └─ next slot ─                        │
//! ```
//!
//! `G_BUCKETS[hash_ptr(target)]` heads a singly-linked **hash chain**
//! of `TargetCell`s (entries with hash collisions). Each cell heads a
//! singly-linked list of `ObserverNode`s describing what observed it
//! and how. On observer deregister, the registry shrinks: empty cells
//! are unlinked from their bucket and freed.
//!
//! ## `static mut` 替代 → `AtomicPtr`
//!
//! Same rationale as `torajs-arr::pool` / `torajs-arr::props` — tora
//! runtime is single-threaded today (no thread-spawn surface), but
//! Rust 2024's `static_mut_refs` lint deprecates raw `static mut`.
//! `AtomicPtr` + `Ordering::Relaxed` compiles to identical loads/stores
//! on every target (x86_64 / aarch64) while keeping `&'static` APIs
//! sound, and pre-pays the multi-threaded story for future Workers.
//!
//! Intrusive list pointers (`TargetCell::observers`, `TargetCell::next`,
//! `ObserverNode::next`) live inside heap nodes, NOT in `static` — so
//! plain `*mut` is fine there; no Atomic wrapper needed.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crate::layout::{OBSERVER_WEAKMAP, OBSERVER_WEAKREF, OBSERVER_WEAKSET, WeakRef};

/// Number of hash-chain heads. MUST be a power of 2 — `hash_ptr`
/// uses `WEAKREF_BUCKETS - 1` as the bit mask. Matches
/// `runtime_weakref.c::WEAKREF_BUCKETS`.
pub const WEAKREF_BUCKETS: usize = 1024;

// Compile-time guard: hash mask requires power-of-2 bucket count.
const _: () = assert!(WEAKREF_BUCKETS.is_power_of_two());

/// One link in a `TargetCell`'s observer list. Field layout MUST match
/// `runtime_weakref.c::ObserverNode` exactly so an `ObserverNode*`
/// returned from C-side code (none today, but future invalidate-hook
/// ports may walk the list) reads the right bytes. `#[repr(C)]` plus
/// `u32` for `kind` (mirrors C `enum ObserverKind` storage, which the
/// compiler emits as `int`).
#[repr(C)]
struct ObserverNode {
    kind: u32,
    owner: *mut c_void,
    next: *mut ObserverNode,
}

/// One entry in a bucket's hash chain. Field layout matches
/// `runtime_weakref.c::TargetCell`. Each TargetCell carries the
/// observed target pointer + the head of its observer list + the
/// next cell in the same bucket's hash chain.
#[repr(C)]
struct TargetCell {
    target: *mut c_void,
    observers: *mut ObserverNode,
    next: *mut TargetCell,
}

/// `target → bucket-head TargetCell*` table. `static mut` replacement
/// (see module docs). Zero-initialized at program load.
static G_BUCKETS: [AtomicPtr<TargetCell>; WEAKREF_BUCKETS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; WEAKREF_BUCKETS];

/// Total live observer count across all cells. `target_dying`'s
/// fast-path short-circuit reads this first; the common case
/// (no live WeakRef/Map/Set in the whole program) skips the hash
/// lookup entirely. Matches `runtime_weakref.c::g_active`.
static G_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

    /// Defined in `runtime_weakmap.c`. Called from `target_dying`
    /// when an observer dispatching as `OBSERVER_WEAKMAP` fires;
    /// the owning WeakMap evicts the entry keyed by `dying_key`.
    /// Stays C-side until P4.3'-c ports `runtime_weakmap.c`.
    fn __torajs_weakmap_invalidate_key(owner: *mut c_void, dying_key: *mut c_void);

    /// Defined in `runtime_weakset.c`. Mirror of weakmap variant.
    /// Stays C-side until P4.3'-d ports `runtime_weakset.c`.
    fn __torajs_weakset_invalidate_key(owner: *mut c_void, dying_key: *mut c_void);
}

/// Fold a pointer into a bucket index. Same constants + steps as the
/// C `hash_ptr`. Two splitmix-style multiplies + xor-shift give a good
/// distribution on real-world `malloc` addresses (which cluster on
/// alignment + arena chunks). `& (WEAKREF_BUCKETS - 1)` is safe because
/// the bucket count is power-of-2 (compile-time-asserted above).
#[inline]
fn hash_ptr(p: *mut c_void) -> usize {
    let mut v = p as usize;
    v ^= v >> 33;
    v = v.wrapping_mul(0xff51_afd7_ed55_8ccd);
    v ^= v >> 33;
    v = v.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    v ^= v >> 33;
    v & (WEAKREF_BUCKETS - 1)
}

/// Walk bucket `bkt`'s hash chain looking for the cell whose target
/// matches; returns NULL when there's no live cell for `target`.
///
/// # Safety
/// The chain is single-thread-mutated only via this module; no other
/// code may set `G_BUCKETS[bkt]` or walk `.next` concurrently.
#[inline]
unsafe fn registry_find(target: *mut c_void, bkt: usize) -> *mut TargetCell {
    let mut cur = G_BUCKETS[bkt].load(Ordering::Relaxed);
    while !cur.is_null() {
        if unsafe { (*cur).target } == target {
            return cur;
        }
        cur = unsafe { (*cur).next };
    }
    ptr::null_mut()
}

/// Find or alloc the TargetCell for `target`. On a fresh cell, the
/// new entry is pushed to the front of the bucket's chain (LIFO —
/// recently-touched cells get found first; matches the C code).
///
/// # Safety
/// Same single-thread invariant as `registry_find`.
#[inline]
unsafe fn registry_get_or_alloc(target: *mut c_void, bkt: usize) -> *mut TargetCell {
    let existing = unsafe { registry_find(target, bkt) };
    if !existing.is_null() {
        return existing;
    }
    let c = unsafe { malloc(core::mem::size_of::<TargetCell>()) } as *mut TargetCell;
    unsafe {
        (*c).target = target;
        (*c).observers = ptr::null_mut();
        (*c).next = G_BUCKETS[bkt].load(Ordering::Relaxed);
    }
    G_BUCKETS[bkt].store(c, Ordering::Relaxed);
    c
}

/// Unlink `cell` from bucket `bkt`'s hash chain and free it. Searches
/// the chain for the cell pointer (not its target) since collision
/// chains are short (≤ a few entries on typical workloads).
///
/// # Safety
/// `cell` must be a live entry in `G_BUCKETS[bkt]`. After return,
/// `cell` is freed and must not be touched.
#[inline]
unsafe fn registry_remove_cell(cell: *mut TargetCell, bkt: usize) {
    let mut slot: *mut AtomicPtr<TargetCell> = &G_BUCKETS[bkt] as *const _ as *mut _;
    let mut cur = unsafe { (*slot).load(Ordering::Relaxed) };
    while !cur.is_null() {
        if cur == cell {
            unsafe { (*slot).store((*cur).next, Ordering::Relaxed) };
            break;
        }
        // Move slot to point at `cur->next`'s address (still a single-
        // pointer cell; we just rewrite it via plain `*mut` since the
        // intrusive `next` is not Atomic).
        slot = unsafe { &raw mut (*cur).next as *mut AtomicPtr<TargetCell> };
        cur = unsafe { (*cur).next };
    }
    unsafe { free(cell as *mut c_void) };
}

// ============================================================
// Public (C-callable) API.
// ============================================================

/// Add an observer of `kind` owned by `owner` against `target`.
/// Called from `torajs-weak`'s WeakRef/Map/Set create paths and from
/// WeakMap/Set add paths. Tolerant: NULL target = silent no-op (matches
/// C contract; matches the WeakRef-create-with-NULL-target case).
///
/// # Safety
/// `owner` must be the heap pointer of the observer. The observer
/// must call `__torajs_weakref_registry_deregister` with the same
/// (target, kind, owner) tuple before the observer is freed if the
/// target has not yet died.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_registry_register(
    target: *mut c_void,
    kind: u32,
    owner: *mut c_void,
) {
    if target.is_null() {
        return;
    }
    let bkt = hash_ptr(target);
    let c = unsafe { registry_get_or_alloc(target, bkt) };
    let n = unsafe { malloc(core::mem::size_of::<ObserverNode>()) } as *mut ObserverNode;
    unsafe {
        (*n).kind = kind;
        (*n).owner = owner;
        (*n).next = (*c).observers;
        (*c).observers = n;
    }
    G_ACTIVE.fetch_add(1, Ordering::Relaxed);
}

/// Remove the first matching (kind, owner) observer entry from
/// `target`'s cell. Tolerant: NULL target / missing cell / missing
/// (kind, owner) tuple all silently no-op. After removal, if the cell
/// has no observers left, it's unlinked from its bucket and freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_registry_deregister(
    target: *mut c_void,
    kind: u32,
    owner: *mut c_void,
) {
    if target.is_null() {
        return;
    }
    let bkt = hash_ptr(target);
    let c = unsafe { registry_find(target, bkt) };
    if c.is_null() {
        return;
    }
    // Walk the observer list. `slot` points at the place that
    // currently holds the pointer to the candidate node; if we
    // find a match, we rewrite `*slot` to skip past it.
    let mut slot: *mut *mut ObserverNode = unsafe { &raw mut (*c).observers };
    while !unsafe { *slot }.is_null() {
        let cur = unsafe { *slot };
        if unsafe { (*cur).kind } == kind && unsafe { (*cur).owner } == owner {
            unsafe {
                *slot = (*cur).next;
                free(cur as *mut c_void);
            }
            G_ACTIVE.fetch_sub(1, Ordering::Relaxed);
            break;
        }
        slot = unsafe { &raw mut (*cur).next };
    }
    if unsafe { (*c).observers }.is_null() {
        unsafe { registry_remove_cell(c, bkt) };
    }
}

/// Broadcast cleanup walk for a target whose strong rc transitioned
/// to zero. Called from `__torajs_rc_dec` and the inlined Obj-drop
/// walk in `ssa_lower`. Per-observer-kind dispatch:
///
///   - `OBSERVER_WEAKREF` — clear the WeakRef's `target` slot to NULL.
///   - `OBSERVER_WEAKMAP` — call `__torajs_weakmap_invalidate_key`.
///   - `OBSERVER_WEAKSET` — call `__torajs_weakset_invalidate_key`.
///
/// Cells / nodes are freed as the walk proceeds, then the bucket is
/// unlinked. The very common case "no live observers in this program"
/// short-circuits on the `G_ACTIVE == 0` check before touching any
/// bucket — keeps the hot drop path overhead-free in programs without
/// any weak references.
///
/// # Safety
/// Each `owner` pointer in the bucket must still point to a live
/// observer (`WeakRef` / `WeakMap` / `WeakSet`) — this is the
/// register/deregister contract. After this call, the registry no
/// longer has any entries for `target`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(target: *mut c_void) {
    if G_ACTIVE.load(Ordering::Relaxed) == 0 {
        return;
    }
    let bkt = hash_ptr(target);
    let c = unsafe { registry_find(target, bkt) };
    if c.is_null() {
        return;
    }
    let mut cur = unsafe { (*c).observers };
    while !cur.is_null() {
        let kind = unsafe { (*cur).kind };
        let owner = unsafe { (*cur).owner };
        match kind {
            OBSERVER_WEAKREF => {
                // Clear the WeakRef's `target` slot — subsequent
                // `deref()` returns NULL. ABI-shared layout means the
                // offset is fixed; same byte as runtime_weakref.c's
                // `((WeakRef *)cur->owner)->target = NULL`.
                unsafe { (*(owner as *mut WeakRef)).target = ptr::null_mut() };
            }
            OBSERVER_WEAKMAP => unsafe { __torajs_weakmap_invalidate_key(owner, target) },
            OBSERVER_WEAKSET => unsafe { __torajs_weakset_invalidate_key(owner, target) },
            _ => {
                // Unknown observer kind — registry is corrupt; drop
                // the node silently (matches C's no-default switch
                // behavior: fallthrough to the free below).
            }
        }
        let next = unsafe { (*cur).next };
        unsafe { free(cur as *mut c_void) };
        G_ACTIVE.fetch_sub(1, Ordering::Relaxed);
        cur = next;
    }
    unsafe { registry_remove_cell(c, bkt) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layouts_match_c() {
        // ObserverNode: u32 + 4B pad + ptr + ptr = 24B, align 8.
        assert_eq!(core::mem::size_of::<ObserverNode>(), 24);
        assert_eq!(core::mem::align_of::<ObserverNode>(), 8);
        assert_eq!(core::mem::offset_of!(ObserverNode, kind), 0);
        assert_eq!(core::mem::offset_of!(ObserverNode, owner), 8);
        assert_eq!(core::mem::offset_of!(ObserverNode, next), 16);

        // TargetCell: 3×ptr = 24B, align 8.
        assert_eq!(core::mem::size_of::<TargetCell>(), 24);
        assert_eq!(core::mem::align_of::<TargetCell>(), 8);
        assert_eq!(core::mem::offset_of!(TargetCell, target), 0);
        assert_eq!(core::mem::offset_of!(TargetCell, observers), 8);
        assert_eq!(core::mem::offset_of!(TargetCell, next), 16);
    }

    #[test]
    fn hash_ptr_is_bucket_index() {
        // Smoke: any in-bounds index. Distribution quality isn't a
        // test invariant; that's runtime perf, not correctness.
        for &p in &[0x1usize, 0x100, 0xdead_beef, 0xffff_ffff_ffff_fff0] {
            let h = hash_ptr(p as *mut c_void);
            assert!(h < WEAKREF_BUCKETS);
        }
    }
}
