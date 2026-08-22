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
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_object_proto_install(proto: *mut c_void);
    fn __torajs_function_proto_install(proto: *mut c_void);
    fn __torajs_iterator_proto_install(proto: *mut c_void);
    fn __torajs_proto_tostringtag_install(proto: *mut c_void, idx: i64);
    fn __torajs_proto_symbol_keys_install(proto: *mut c_void, idx: i64);
}

/// Number of builtin prototypes ssa_lower can request. Order is
/// fixed by the tag constants ssa_lower emits — never reorder
/// (append-only; AsyncFunction joined as 14, RFC 20260721 刀 4;
/// Iterator joined as 15, RFC 20260730-iterator-global 刀 1;
/// WeakMap 16 / WeakSet 17 / WeakRef 18 joined in rotation 314 —
/// the first two already had instances and any-lane dispatch and
/// were missing only the constructor's VALUE face; WeakRef needed
/// its any-lane arm too. ArrayBuffer 19 and the eleven typed arrays
/// 20-30 joined for RFC 20260823-typedarray-substrate 刀 4 — the
/// twelve are in `torajs_buffer::typedarray::Kind` order after the
/// buffer, because that is the order the discriminant already
/// fixes and a second ordering would be a second thing to keep in
/// step).
pub const NUM_BUILTIN_PROTOS: usize = 32;

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
        // §25.1.4.1 ArrayBuffer takes (length, options) and declares
        // length 1; every §23.2.5 typed-array constructor declares 3.
        19 => ("ArrayBuffer", 1),
        20 => ("Int8Array", 3),
        21 => ("Uint8Array", 3),
        22 => ("Uint8ClampedArray", 3),
        23 => ("Int16Array", 3),
        24 => ("Uint16Array", 3),
        25 => ("Int32Array", 3),
        26 => ("Uint32Array", 3),
        27 => ("Float32Array", 3),
        28 => ("Float64Array", 3),
        29 => ("BigInt64Array", 3),
        30 => ("BigUint64Array", 3),
        31 => ("Float16Array", 3),
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

/// `Map.prototype`'s slot — its reified methods brand-check
/// thisMapObject (§24.1.3) on borrow re-dispatch.
pub const MAP_PROTO_TAG: usize = 11;

/// `Set.prototype`'s slot — its reified methods brand-check
/// thisSetObject (§24.2.3) on borrow re-dispatch.
pub const SET_PROTO_TAG: usize = 12;

/// `%Iterator.prototype%`'s slot — its mint installs the §27.1.2.1
/// `[Symbol.iterator]` / §27.1.4.1 `[Symbol.dispose]` own entries
/// (see the mint site; RFC 20260809 B6).
pub const ITERATOR_PROTO_TAG: usize = 15;

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
    // %Iterator.prototype% carries the §27.1.2.1 [@@iterator]
    // return-this and §27.1.4.1 [@@dispose] own entries — the faces
    // a generator instance inherits through its real prototype chain
    // (RFC 20260809 B6). Same pre-CAS posture as the installs above.
    if idx == ITERATOR_PROTO_TAG {
        unsafe { __torajs_iterator_proto_install(fresh) };
    }
    // Eight prototypes carry a §20.4.3.5-shaped `@@toStringTag` own
    // entry (Symbol / BigInt / Promise / Map / Set / WeakMap /
    // WeakSet / WeakRef); the install is a no-op for the rest, which
    // the spec gives no tag. Same pre-CAS posture as the three above.
    // Array / Map / Set / String carry `[Symbol.iterator]` aliased to a
    // named method of theirs (§23.1.3.40 / §24.1.3.14 / §24.2.3.13 /
    // §22.1.3.36) and Array.prototype also §23.1.3.44 `@@unscopables`
    // (its entries land in the Arr side props — §23.1.3 makes it an
    // array exotic object); a no-op for the rest, posture as above.
    //
    // BEFORE the tag install, not after: §10.1.11.1 lists own symbol
    // keys in creation order, and the spec clauses run @@iterator
    // first (§24.1.3.14 before §24.1.3.15, §24.2.3.13 before
    // §24.2.3.14), which is the order `getOwnPropertySymbols` must
    // answer in.
    unsafe { __torajs_proto_symbol_keys_install(fresh, idx as i64) };
    unsafe { __torajs_proto_tostringtag_install(fresh, idx as i64) };
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

/// §20.5 Error-family class registry — the injected `Error` /
/// NativeError CLASS objects, keyed by a fixed family index, so the
/// globalThis fill (torajs-anyvalue) can answer the SAME identity the
/// bare name reads (`globalThis.TypeError === TypeError`).
///
/// The recorder is torajs-meta's `__torajs_error_proto_install` — the
/// one runtime site that sees both the class NAME and its registered
/// tag; it reaches the store through the `_record` C symbol (meta
/// keeps a zero-Cargo-dep tree). A program that never injects a
/// family member leaves its slot 0 and the dynamic read keeps the
/// fill's loud MISSING_KNOWN posture. The classes live in
/// module-scope let bindings (process lifetime), so the slot holds a
/// borrowed immediate like `CLASSES_BY_TAG_IMM` does.
mod patch_bits;

use patch_bits::ANY_MINTED;

// The masks moved to the child, but callers across the workspace
// reach them as `torajs_rc::builtin_proto::<fn>` — keep that path
// answering so the split stays invisible outside this file.
pub use patch_bits::{
    __torajs_builtin_proto_has_patch, __torajs_builtin_proto_is_deleted,
    __torajs_builtin_proto_is_shadowed, __torajs_builtin_proto_mark_deleted,
    __torajs_builtin_proto_note_own_write,
};

pub mod native_error_class {
    use core::sync::atomic::{AtomicU64, Ordering};

    /// Fixed family order — the index is ABI between the recorder
    /// (torajs-meta `classmeta/error_family.rs` mirrors this list)
    /// and the reader (torajs-anyvalue globalThis fill). Append-only.
    pub const NATIVE_ERROR_FAMILY: [&str; 9] = [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "EvalError",
        "URIError",
        "AggregateError",
        "SuppressedError",
    ];

    static CLASS_ANYVS: [AtomicU64; NATIVE_ERROR_FAMILY.len()] =
        [const { AtomicU64::new(0) }; NATIVE_ERROR_FAMILY.len()];

    /// Record the class-object AnyValue immediate under family index
    /// `idx`. Out-of-range indexes are ignored (defensive — the
    /// recorder derives `idx` from its own mirror of the family
    /// list).
    #[unsafe(no_mangle)]
    pub extern "C" fn __torajs_native_error_class_record(idx: i64, class_anyv: u64) {
        if idx < 0 || idx as usize >= CLASS_ANYVS.len() {
            return;
        }
        CLASS_ANYVS[idx as usize].store(class_anyv, Ordering::Release);
    }

    /// The registered class-object AnyValue for family index `idx`,
    /// 0 when this program never injected that class.
    pub fn native_error_class(idx: usize) -> u64 {
        CLASS_ANYVS
            .get(idx)
            .map_or(0, |slot| slot.load(Ordering::Acquire))
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

// The @@toStringTag install lives in torajs-meta (linked into `tr`);
// same posture as the stubs above.
#[cfg(test)]
unsafe fn __torajs_proto_tostringtag_install(_proto: *mut c_void, _idx: i64) {}

// The @@iterator / @@dispose install lives in torajs-anyvalue
// (linked into `tr`); same posture as the two stubs above.
#[cfg(test)]
unsafe fn __torajs_iterator_proto_install(_proto: *mut c_void) {}

// The prototype-side well-known-symbol installs (@@iterator alias,
// @@unscopables) are torajs-anyvalue's too; same posture.
#[cfg(test)]
unsafe fn __torajs_proto_symbol_keys_install(_proto: *mut c_void, _idx: i64) {}

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
