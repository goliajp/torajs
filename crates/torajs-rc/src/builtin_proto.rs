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
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_object_proto_install(proto: *mut c_void);
    fn __torajs_function_proto_install(proto: *mut c_void);
}

/// Number of builtin prototypes ssa_lower can request. Order is
/// fixed by the tag constants ssa_lower emits — never reorder.
pub const NUM_BUILTIN_PROTOS: usize = 14;

/// `Array.prototype`'s slot. ES §23.1.3 makes it an *Array exotic
/// object* (an empty one) rather than an ordinary object, so its
/// singleton is a real `Arr` cell — that is what makes
/// `Array.isArray(Array.prototype)` true, `Array.prototype.length`
/// 0, and `Array.prototype.toString()` "" (an empty join) instead of
/// the inherited `[object Object]`. Every consumer reaching the cell
/// through the Any lane then routes on its heap tag and lands in the
/// array arm for free; the ones that read a prototype's own
/// properties branch on the cell shape (`method_support_proto`,
/// `prop_has`).
pub const ARRAY_PROTO_TAG: usize = 2;

/// `Object.prototype`'s slot — its mint additionally installs the
/// Annex B `__proto__` accessor own entry (see the mint site).
pub const OBJECT_PROTO_TAG: usize = 1;

/// `Function.prototype`'s slot — its mint installs the §10.2.4
/// restricted-property accessors (see the mint site).
pub const FUNCTION_PROTO_TAG: usize = 13;

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
/// First call per tag allocates the cell the spec asks for —
/// an empty `Arr` for [`ARRAY_PROTO_TAG`], a dynobj for the rest —
/// and CAS-installs its address into the slot; subsequent calls
/// return the cached pointer so `<Ctor>.prototype ===
/// <Ctor>.prototype` is `true` (spec singleton identity).
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
    // SAFETY: both externs are wired to the runtime allocators
    // (torajs-dynobj / torajs-arr) when linked into the final
    // binary. Cargo-test builds in this crate use the unique-address
    // stubs below.
    let fresh = if idx == ARRAY_PROTO_TAG {
        unsafe { __torajs_arr_alloc_any(0) as *mut c_void }
    } else {
        unsafe { __torajs_dynobj_alloc() }
    };
    // Process-lifetime singleton — immortal, rc traffic no-ops
    // (RFC 20260718-accessor-reify 刀 1). The `<Ctor>.prototype`
    // lowering hands out BORROW boxes of this cell; consumer sites
    // drop args under the owned convention, so a mortal singleton
    // bleeds refcount until it frees and a later allocation reuses
    // the address — at which point `__torajs_builtin_proto_tag_of`
    // misclassifies the newcomer as the prototype (diag: a fresh
    // `{}` answered null for its own [[Prototype]]).
    unsafe {
        let flags = fresh.cast::<u8>().add(6).cast::<u16>();
        flags.write(flags.read() | crate::FLAG_STATIC_LITERAL);
    }
    // %Object.prototype% carries the Annex B §B.2.2.1 `__proto__`
    // accessor as a real own entry (RFC 20260718-accessor-reify
    // 刀 1) — installed on the fresh cell before the CAS so a
    // race loser leaks a fully-formed dynobj (same benign posture
    // as the allocation itself).
    if idx == OBJECT_PROTO_TAG {
        unsafe { __torajs_object_proto_install(fresh) };
    }
    // %Function.prototype% carries the §10.2.4 restricted-property
    // accessors (`caller` / `arguments`, all four faces the ONE
    // %ThrowTypeError% cell) — same posture as the tag-1 install.
    if idx == FUNCTION_PROTO_TAG {
        unsafe { __torajs_function_proto_install(fresh) };
    }
    let fresh_addr = fresh as usize;
    match slot.compare_exchange(0, fresh_addr, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => fresh,
        Err(winner) => winner as *mut c_void,
    }
}

/// Reverse lookup — the builtin tag whose `.prototype` singleton is
/// `p`, or `-1` when `p` is not one (never-allocated protos can't
/// match: their slots still hold 0). O(14) scan; callers are cold
/// reflection probes (own-property / descriptor synthesis, RFC
/// 20260712).
///
/// # Safety
/// `p` is any pointer value — only compared, never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_tag_of(p: *const c_void) -> i64 {
    if p.is_null() {
        return -1;
    }
    let addr = p as usize;
    for (i, slot) in SLOTS.iter().enumerate() {
        if slot.load(Ordering::Acquire) == addr {
            return i as i64;
        }
    }
    -1
}

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

// Cargo-test stub for the dynobj_alloc extern. The real symbol lives
// in the runtime substrate (linked into `tr`); unit tests in this
// crate only verify the singleton-CAS logic, so we hand out unique
// non-null addresses from a monotonic counter. Local fn (not
// `#[unsafe(no_mangle)]`) — only this crate's test binary calls it,
// and we don't want to collide with the torajs-meta crate's own
// stub at link time.
#[cfg(test)]
unsafe fn __torajs_dynobj_alloc() -> *mut c_void {
    // Real (leaked) allocation — the mint path writes the header's
    // static-literal flag, so the address must be dereferenceable
    // (the old monotonic-counter fake address SIGSEGV'd there).
    Box::into_raw(Box::new([0u8; 64])) as *mut c_void
}

// Same for the Array-prototype cell — the singleton logic under
// test only cares that the address is unique and non-null.
#[cfg(test)]
unsafe fn __torajs_arr_alloc_any(_cap: u64) -> *mut u8 {
    unsafe { __torajs_dynobj_alloc() as *mut u8 }
}

// The `__proto__` accessor install lives in torajs-meta (linked
// into `tr`); the singleton logic under test doesn't observe it.
#[cfg(test)]
unsafe fn __torajs_object_proto_install(_proto: *mut c_void) {}

#[cfg(test)]
unsafe fn __torajs_function_proto_install(_proto: *mut c_void) {}

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
    fn tag_of_round_trips_and_rejects_strangers() {
        let p3 = unsafe { __torajs_get_builtin_prototype(3) };
        let p8 = unsafe { __torajs_get_builtin_prototype(8) };
        assert_eq!(unsafe { __torajs_builtin_proto_tag_of(p3) }, 3);
        assert_eq!(unsafe { __torajs_builtin_proto_tag_of(p8) }, 8);
        assert_eq!(
            unsafe { __torajs_builtin_proto_tag_of(core::ptr::null()) },
            -1
        );
        assert_eq!(
            unsafe { __torajs_builtin_proto_tag_of(0xDEAD_BEE0 as *const c_void) },
            -1
        );
    }

    #[test]
    fn deleted_mask_marks_per_tag_per_mid() {
        assert_eq!(unsafe { __torajs_builtin_proto_is_deleted(3, 95) }, 0);
        unsafe { __torajs_builtin_proto_mark_deleted(3, 95) };
        assert_eq!(unsafe { __torajs_builtin_proto_is_deleted(3, 95) }, 1);
        // other tag / other mid unaffected; idempotent re-mark.
        assert_eq!(unsafe { __torajs_builtin_proto_is_deleted(0, 95) }, 0);
        assert_eq!(unsafe { __torajs_builtin_proto_is_deleted(3, 94) }, 0);
        unsafe { __torajs_builtin_proto_mark_deleted(3, 95) };
        assert_eq!(unsafe { __torajs_builtin_proto_is_deleted(3, 95) }, 1);
        // out-of-range ignored / answers 0.
        unsafe { __torajs_builtin_proto_mark_deleted(99, 5) };
        unsafe { __torajs_builtin_proto_mark_deleted(3, 300) };
        assert_eq!(unsafe { __torajs_builtin_proto_is_deleted(99, 5) }, 0);
        assert_eq!(unsafe { __torajs_builtin_proto_is_deleted(3, 300) }, 0);
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
