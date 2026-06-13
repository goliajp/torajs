//! Builtin-prototype singleton substrate.
//!
//! `<Ctor>.prototype` (Number / Object / Array / String / Boolean /
//! Symbol / BigInt / RegExp / Date / Error / Promise / Map / Set /
//! Function) emits one lazy-init dynobj per builtin, reused across
//! the whole program. Closes the identity wedge introduced when
//! cases#obj-is-frozen-any-segv switched `<Ctor>.prototype` emit from
//! a NaN-box `VALUE_NULL` sentinel to a fresh `__torajs_dynobj_alloc`
//! per access — that fix made `Number.prototype === Number.prototype`
//! return `false` because each access allocated a new dynobj. Per
//! ES2015 §19.x, each builtin's `.prototype` is one well-known object;
//! a real fix needs a singleton, which is what this module provides.
//!
//! ssa_lower now emits
//! `__torajs_get_builtin_prototype(<tag>)`
//! at every `<Ctor>.prototype` access; the first call per tag
//! allocates the dynobj, every subsequent call returns the same
//! pointer.
//!
//! ## Multi-thread-ready
//!
//! Per `torajs-design-principles.md` §6.2 (no-GC + multi-thread-ready
//! substrate), each slot is an `AtomicUsize` (internal atomic) rather
//! than a `lazy_static<Mutex<_>>`. A benign double-init race (CAS
//! loser leaks one fresh dynobj) is acceptable here — this fn fires
//! at most ~14 times per program lifetime, dwarfed by everything else.

use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
}

/// Number of builtin prototypes ssa_lower can request. Order is
/// fixed by the tag constants ssa_lower emits — never reorder.
pub const NUM_BUILTIN_PROTOS: usize = 14;

// One AtomicUsize slot per builtin tag. Initialized to 0 (= "not yet
// allocated"); `__torajs_get_builtin_prototype` CAS-installs the
// first non-null pointer.
//
// `*mut c_void` is `!Sync`, so we store the pointer's address as
// `usize` and round-trip the cast at each access. This keeps the
// shared state `Sync` while preserving the raw-pointer ABI we
// expose to ssa_lower-emitted IR.
#[allow(clippy::declare_interior_mutable_const)]
const SLOT_INIT: AtomicUsize = AtomicUsize::new(0);
static SLOTS: [AtomicUsize; NUM_BUILTIN_PROTOS] = [SLOT_INIT; NUM_BUILTIN_PROTOS];

/// Lazy-init singleton for a builtin's `.prototype`.
///
/// `tag` ∈ \[0, 14): Number=0, Object=1, Array=2, String=3,
/// Boolean=4, Symbol=5, BigInt=6, RegExp=7, Date=8, Error=9,
/// Promise=10, Map=11, Set=12, Function=13. ssa_lower must emit one
/// of these via `Operand::ConstI64(<tag>)`. Out-of-range `tag`
/// returns NULL (defensive — ssa_lower should never emit it).
///
/// First call per tag allocates a fresh dynobj via
/// `__torajs_dynobj_alloc()` and CAS-installs its address into the
/// slot; subsequent calls return the cached pointer so
/// `<Ctor>.prototype === <Ctor>.prototype` is `true` (spec
/// singleton identity).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void {
    let idx = tag as usize;
    if idx >= NUM_BUILTIN_PROTOS {
        return core::ptr::null_mut();
    }
    let slot = &SLOTS[idx];
    let cached = slot.load(Ordering::Acquire);
    if cached != 0 {
        return cached as *mut c_void;
    }
    // SAFETY: extern is wired to the runtime dynobj allocator
    // (`crates/torajs-meta` / `runtime_*.c` impl) when linked into
    // the final binary. Cargo-test builds in this crate use the
    // unique-address stub below.
    let fresh = unsafe { __torajs_dynobj_alloc() };
    let fresh_addr = fresh as usize;
    match slot.compare_exchange(0, fresh_addr, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => fresh,
        Err(winner) => winner as *mut c_void,
    }
}

// Cargo-test stub for the dynobj_alloc extern. The real symbol lives
// in the runtime substrate (linked into `tr`); unit tests in this
// crate only verify the singleton-CAS logic, so we hand out unique
// non-null addresses from a monotonic counter. Local fn (not
// `#[unsafe(no_mangle)]`) — only this crate's test binary calls it,
// and we don't want to collide with the torajs-meta crate's own
// stub at link time.
#[cfg(test)]
unsafe fn __torajs_dynobj_alloc() -> *mut c_void {
    static NEXT_ADDR: AtomicUsize = AtomicUsize::new(0x1000);
    NEXT_ADDR.fetch_add(0x10, Ordering::SeqCst) as *mut c_void
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests share global `SLOTS` — sequence them through a single
    // entry to make the cache-state expectations deterministic.
    // `cargo nextest` already gives per-test process isolation, so
    // each test sees a fresh `SLOTS`.

    #[test]
    fn same_tag_returns_same_ptr() {
        let p1 = unsafe { __torajs_get_builtin_prototype(0) };
        let p2 = unsafe { __torajs_get_builtin_prototype(0) };
        assert!(!p1.is_null());
        assert_eq!(p1, p2);
    }

    #[test]
    fn different_tags_different_ptrs() {
        let p_number = unsafe { __torajs_get_builtin_prototype(0) };
        let p_object = unsafe { __torajs_get_builtin_prototype(1) };
        assert!(!p_number.is_null());
        assert!(!p_object.is_null());
        assert_ne!(p_number, p_object);
    }

    #[test]
    fn all_tags_allocate_distinct_ptrs() {
        let ptrs: Vec<*mut c_void> = (0..NUM_BUILTIN_PROTOS as i64)
            .map(|t| unsafe { __torajs_get_builtin_prototype(t) })
            .collect();
        for p in &ptrs {
            assert!(!p.is_null());
        }
        for i in 0..ptrs.len() {
            for j in (i + 1)..ptrs.len() {
                assert_ne!(ptrs[i], ptrs[j], "tag {i} and {j} collide");
            }
        }
    }

    #[test]
    fn out_of_range_tag_returns_null() {
        let p_low = unsafe { __torajs_get_builtin_prototype(NUM_BUILTIN_PROTOS as i64) };
        let p_high = unsafe { __torajs_get_builtin_prototype(999) };
        let p_neg = unsafe { __torajs_get_builtin_prototype(-1) };
        assert!(p_low.is_null());
        assert!(p_high.is_null());
        assert!(p_neg.is_null());
    }

    #[test]
    fn second_access_after_cache_returns_cached() {
        // Prime tag 5.
        let first = unsafe { __torajs_get_builtin_prototype(5) };
        // Burn allocator state by hitting other tags.
        for t in [0i64, 1, 2, 3, 4, 6, 7] {
            let _ = unsafe { __torajs_get_builtin_prototype(t) };
        }
        // Tag 5 must still resolve to the same pointer.
        let second = unsafe { __torajs_get_builtin_prototype(5) };
        assert_eq!(first, second);
    }
}
