//! `__torajs_value_drop_heap` — universal heap-typed drop dispatch.
//!
//! Layer-1 substrate (P7.i-drop, 2026-05-24) — replaces the
//! `__torajs_value_drop_heap` in `runtime_str.c`. Single fn (the
//! "dispatch table" exception in `file-size.md` applies even though
//! the body is small — the file does one logical thing). Reads the
//! universal heap header's `type_tag` and routes to the per-type
//! `__torajs_*_drop` extern. Used by:
//!
//! - `__torajs_anyv_rc_dec` when an AnyValue cell hits rc 0
//! - `__torajs_arr_drop_any` when an Array<Any> slot is ANY_HEAP
//! - dynobj entry drop (key Str + value child)
//!
//! ## Cross-tier ABI
//!
//! Calls resolve at `tr build` link time against the matching
//! sibling staticlib:
//!
//! | tag             | extern                          | provider          |
//! |-----------------|---------------------------------|-------------------|
//! | `Str`           | `__torajs_str_drop`             | torajs-str        |
//! | `Arr`           | `__torajs_arr_drop`             | torajs-arr        |
//! | `Response`      | `__torajs_response_drop`        | torajs-fetch      |
//! | `BigInt`        | `__torajs_bigint_drop`          | torajs-bigint     |
//! | `WeakRef`       | `__torajs_weakref_drop`         | torajs-weak       |
//! | `WeakMap`       | `__torajs_weakmap_drop`         | torajs-weak       |
//! | `WeakSet`       | `__torajs_weakset_drop`         | torajs-weak       |
//! | `Map`           | `__torajs_map_drop`             | torajs-collections|
//! | `MapIter`       | `__torajs_map_iter_drop`        | torajs-collections|
//! | `ArrIter`       | `__torajs_arr_iter_drop`        | torajs-arr        |
//! | `DynObj`        | `__torajs_dynobj_drop`          | torajs-dynobj     |
//! | (other)         | `__torajs_rc_dec` + libc `free` | torajs-rc + libc  |
//!
//! Fallback (Obj / Substr / Closure / RegExp / Date): rc-dec;
//! `free` on rc==0. May leak inner refs for types with nested heap
//! children — V3-10.b tightens this through the per-type drop hooks
//! at the call site (array element walks, dynobj entry walks, etc.).
//!
//! `Response` is gated `#[cfg(not(target_os = "wasi"))]` (no libcurl
//! on WASI; mirrors runtime_str.c's `#ifndef __wasi__` gate).

use core::ffi::c_void;

use torajs_rc::{__torajs_rc_dec, Tag};

// v0.7-A2 step 6b — force-link mmalloc for the `__torajs_libc_free`
// extern below.
extern crate torajs_mmalloc as _;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat free — v0.7-A2 step 6b finale.
    /// Fallback free for Closure / RegExp / Date / Obj heap (when
    /// the type-specific _drop arm isn't in the match above). Every
    /// such heap's allocator must already be mmalloc by this cut.
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_arr_drop(p: *mut c_void);
    fn __torajs_bigint_drop(p: *mut c_void);
    fn __torajs_weakref_drop(p: *mut c_void);
    fn __torajs_weakmap_drop(p: *mut c_void);
    fn __torajs_weakset_drop(p: *mut c_void);
    fn __torajs_map_drop(p: *mut c_void);
    fn __torajs_map_iter_drop(p: *mut c_void);
    fn __torajs_arr_iter_drop(p: *mut c_void);
    fn __torajs_dynobj_drop(p: *mut c_void);
}

#[cfg(not(target_os = "wasi"))]
unsafe extern "C" {
    fn __torajs_response_drop(p: *mut c_void);
}

/// Universal heap-typed drop dispatch. NULL is a no-op. Reads
/// `type_tag` at offset +4 in the universal header.
///
/// # Safety
/// `child` is NULL or a valid heap block with the universal
/// `HeapHeader` layout. After the call the block is owned by the
/// matching `_drop` (which may free immediately or pool-recycle).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(child: *mut c_void) {
    if child.is_null() {
        return;
    }
    // Step 7d-A — same NaN-box gate as `__torajs_rc_inc`/`_dec`:
    // when an `Array<Any>` slot or dynobj value carries a NaN-box
    // immediate (Int32 / f64 / Null / Undef / Bool sentinels stored
    // under ANY_HEAP because ssa_lower can't statically distinguish
    // post-7d-A), bypass — there's no heap header to dereference.
    // Real heap pointers always pass `is_cell_like` (top 16 bits
    // zero + low TAG_BIT_TYPE_OTHER bit clear).
    if !nan_box_is_cell_like(child) {
        return;
    }
    // type_tag lives at offset +4 in the universal header (after the
    // u32 refcount).
    let tag = unsafe { (child.cast::<u8>().add(4) as *const u16).read() };
    match tag {
        t if t == Tag::Str as u16 => unsafe { __torajs_str_drop(child) },
        t if t == Tag::Arr as u16 => unsafe { __torajs_arr_drop(child) },
        #[cfg(not(target_os = "wasi"))]
        t if t == Tag::Response as u16 => unsafe { __torajs_response_drop(child) },
        t if t == Tag::BigInt as u16 => unsafe {
            // BigInt has no inner refs — rc-dec, then drop on hit-zero.
            if __torajs_rc_dec(child) != 0 {
                __torajs_bigint_drop(child);
            }
        },
        t if t == Tag::WeakRef as u16 => unsafe { __torajs_weakref_drop(child) },
        t if t == Tag::WeakMap as u16 => unsafe { __torajs_weakmap_drop(child) },
        t if t == Tag::WeakSet as u16 => unsafe { __torajs_weakset_drop(child) },
        t if t == Tag::Map as u16 => unsafe { __torajs_map_drop(child) },
        t if t == Tag::MapIter as u16 => unsafe { __torajs_map_iter_drop(child) },
        t if t == Tag::ArrIter as u16 => unsafe { __torajs_arr_iter_drop(child) },
        t if t == Tag::DynObj as u16 => unsafe { __torajs_dynobj_drop(child) },
        _ => unsafe {
            // Obj / Substr / Closure / RegExp / Date fallback —
            // rc-dec; on hit-zero free the outer block. May leak inner
            // refs for nested-heap types; V3-10.b call-site walks
            // handle that (per-type drop hooks fire from array element /
            // dynobj entry walks).
            if __torajs_rc_dec(child) != 0 {
                free(child);
            }
        },
    }
}

/// True when `child`'s bit pattern looks like a real heap pointer
/// (top 16 bits zero, low TAG_BIT_TYPE_OTHER bit clear, non-zero).
/// Mirrors `is_cell` in `torajs-anyvalue::nanbox` and the gate in
/// `torajs-rc`'s `__torajs_rc_inc` (Step 7d-A).
#[inline]
fn nan_box_is_cell_like(p: *mut c_void) -> bool {
    // Step 8b-B: tighten weak `(v & TAG_TYPE_NUMBER) == 0` to strict
    // `(v & TOP_16_MASK) == 0` so ShortStr (top16 = 0x0001) values
    // are correctly classified as non-cell. Same tighten as
    // `torajs-anyvalue::nanbox::is_cell`.
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    let v = p as u64;
    v != 0 && (v & TOP_16_MASK) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0
}
