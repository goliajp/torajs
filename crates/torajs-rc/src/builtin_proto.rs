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
/// fixed by the tag constants ssa_lower emits — never reorder
/// (append-only; AsyncFunction joined as 14, RFC 20260721 刀 4;
/// Iterator joined as 15, RFC 20260730-iterator-global 刀 1;
/// WeakMap 16 / WeakSet 17 / WeakRef 18 joined in rotation 314 —
/// the first two already had instances and any-lane dispatch and
/// were missing only the constructor's VALUE face; WeakRef needed
/// its any-lane arm too).
pub const NUM_BUILTIN_PROTOS: usize = 19;

/// ES `name` / ctor-clause `length` of the builtin constructor
/// owning each proto tag (RFC 20260720-ctor-static-reflection 刀 3)
/// — the single source both the lowering's ctor-namespace member
/// fold and the runtime reflection probes read (bun-verified
/// 14/14). Lengths per the ctor clauses: §21.1.1 / §20.1.1 /
/// §23.1.1 / §22.1.1 / §20.3.1 / §20.4.1 (Symbol 0) / §21.2.1 /
/// §22.2.4 (RegExp 2) / §21.4.2 (Date 7) / §20.5.1 / §27.2.3 /
/// §24.1.1 (Map 0) / §24.2.2 (Set 0) / §20.2.1 / §27.7.1
/// (AsyncFunction 1) / §27.1.3.1 (Iterator 0) / §24.3.1
/// (WeakMap 0) / §24.4.1 (WeakSet 0) / §26.1.1 (WeakRef 1).
pub fn builtin_ctor_meta(tag: i64) -> Option<(&'static str, u32)> {
    Some(match tag {
        0 => ("Number", 1),
        1 => ("Object", 1),
        2 => ("Array", 1),
        3 => ("String", 1),
        4 => ("Boolean", 1),
        5 => ("Symbol", 0),
        6 => ("BigInt", 1),
        7 => ("RegExp", 2),
        8 => ("Date", 7),
        9 => ("Error", 1),
        10 => ("Promise", 1),
        11 => ("Map", 0),
        12 => ("Set", 0),
        13 => ("Function", 1),
        14 => ("AsyncFunction", 1),
        15 => ("Iterator", 0),
        16 => ("WeakMap", 0),
        17 => ("WeakSet", 0),
        18 => ("WeakRef", 1),
        _ => return None,
    })
}

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

/// `Number.prototype`'s slot — the one family whose `toString` has
/// an ES `length` different from every other prototype's (§21.1.6.6
/// takes a radix), which `any_method_meta_for` disambiguates.
pub const NUMBER_PROTO_TAG: usize = 0;

/// `String.prototype`'s slot — the family whose reified methods run
/// the §22.1.3 generic ToString(this) coerce on borrow re-dispatch.
pub const STRING_PROTO_TAG: usize = 3;

/// `Boolean.prototype`'s slot — its reified toString / valueOf
/// brand-check thisBooleanValue (§20.3.3) on borrow re-dispatch.
pub const BOOLEAN_PROTO_TAG: usize = 4;

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
/// `tag` ∈ \[0, [`NUM_BUILTIN_PROTOS`]): Number=0, Object=1,
/// Array=2, String=3, Boolean=4, Symbol=5, BigInt=6, RegExp=7,
/// Date=8, Error=9, Promise=10, Map=11, Set=12, Function=13,
/// AsyncFunction=14, Iterator=15, WeakMap=16, WeakSet=17,
/// WeakRef=18.
/// ssa_lower must emit one of these via `Operand::ConstI64(<tag>)`.
/// Out-of-range `tag` returns NULL (defensive — ssa_lower should
/// never emit it).
///
/// First call per tag allocates the cell the spec asks for —
/// an empty `Arr` for [`ARRAY_PROTO_TAG`], a dynobj for the rest —
/// and CAS-installs its address into the slot; subsequent calls
/// return the cached pointer so `<Ctor>.prototype ===
/// <Ctor>.prototype` is `true` (spec singleton identity).
/// Read-only probe of an already-minted prototype singleton — NULL
/// when nothing has forced the mint yet. A monkey-patch write
/// (`Number.prototype.split = …`) necessarily minted the singleton
/// first, so NULL ⇔ the prototype cannot carry a patch; the
/// dispatch-miss consult uses this to stay alloc-free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_peek_builtin_prototype(tag: i64) -> *mut c_void {
    let idx = tag as usize;
    if idx >= NUM_BUILTIN_PROTOS {
        return core::ptr::null_mut();
    }
    SLOTS[idx].load(Ordering::Acquire) as *mut c_void
}

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
    // NOTE the singleton stays a plain mortal cell — an earlier fix
    // marked it FLAG_STATIC_LITERAL for rc-immortality, but that
    // flag also carries the `.rodata` conceptual-immutability
    // semantics (freeze no-op / isFrozen true / preventExtensions
    // no-op), which broke `Object.preventExtensions(Object.prototype)`
    // and the isFrozen built-ins suite. The refcount bleed the flag
    // papered over is fixed at its source instead: the
    // `<Ctor>.prototype` lowering now rc_incs the borrowed slot
    // pointer before boxing (owned Any convention).
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
    // Cheap front gate for the own-write note below — a program that
    // never touched any `<Ctor>.prototype` skips the singleton scan
    // on every dynobj own write with one relaxed load.
    ANY_MINTED.store(1, Ordering::Release);
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

// "Some `<Ctor>.prototype` singleton exists" front gate for the
// own-write note — set once at mint time, read on every dynobj own
// write (RFC 20260721 刀 11 G13).
static ANY_MINTED: AtomicU64 = AtomicU64::new(0);

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
