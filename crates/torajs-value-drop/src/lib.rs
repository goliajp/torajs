//! `__torajs_value_drop_heap` — universal heap-typed drop dispatch.
//!
//! Layer-1 substrate (P7.i-drop, 2026-05-24) — replaces the
//! `__torajs_value_drop_heap` in `runtime_str.c`. Single fn (the
//! "dispatch table" exception in `file-size.md` applies even though
//! the body is small — the file does one logical thing). Reads the
//! universal heap header's `type_tag` and routes to the per-type
//! `__torajs_*_drop` extern.
//!
//! Contract: **release one reference** — every arm (or the per-type
//! drop it delegates to) rc-decs itself and frees on hit-zero. The
//! caller must NOT pre-dec (RFC 20260704 S6: the old `anyv_rc_dec`
//! pre-dec made the arm's dec underflow and leak the block). Used by:
//!
//! - `__torajs_anyv_rc_dec` releasing an AnyValue's cell reference
//! - `__torajs_arr_drop_any` when an Array<Any> slot is ANY_HEAP
//! - `__torajs_arr_drop_heap` walking a typed array's cell slots
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
//! | `Arr`           | `__torajs_arr_drop{,_any,_heap}`| torajs-arr        |
//! | `Response`      | `__torajs_response_drop`        | torajs-fetch      |
//! | `BigInt`        | `__torajs_bigint_drop`          | torajs-bigint     |
//! | `WeakRef`       | `__torajs_weakref_drop`         | torajs-weak       |
//! | `WeakMap`       | `__torajs_weakmap_drop`         | torajs-weak       |
//! | `WeakSet`       | `__torajs_weakset_drop`         | torajs-weak       |
//! | `Map`           | `__torajs_map_drop`             | torajs-collections|
//! | `Set`           | `__torajs_set_drop`             | torajs-collections|
//! | `MapIter`       | `__torajs_map_iter_drop`        | torajs-collections|
//! | `ArrIter`       | `__torajs_arr_iter_drop`        | torajs-arr        |
//! | `IterHelper`    | `__torajs_iter_helper_drop`     | torajs-anyvalue   |
//! | `Proxy`         | `__torajs_proxy_drop`           | torajs-anyvalue   |
//! | `ArrayBuffer`   | `__torajs_arraybuffer_drop`     | torajs-buffer     |
//! | `DynObj`        | `__torajs_dynobj_drop`          | torajs-dynobj     |
//! | `RegExp`        | `__torajs_regex_drop`           | torajs-regex      |
//! | `Date`          | `__torajs_date_drop`            | torajs-date       |
//! | `Symbol`        | `__torajs_symbol_drop`          | torajs-str        |
//! | `Promise`       | `__torajs_promise_drop`         | torajs-promise    |
//! | `Obj`           | `__torajs_obj_drop_rc`          | torajs-cycle      |
//! | `Closure`       | env's `drop_fn` slot at `+16`   | synthesized       |
//! | `NumberWrapper` | `__torajs_number_wrapper_drop`  | torajs-wrapper    |
//! | `StringWrapper` | `__torajs_string_wrapper_drop`  | torajs-wrapper    |
//! | `BooleanWrapper`| `__torajs_boolean_wrapper_drop` | torajs-wrapper    |
//! | `SymbolWrapper` | `__torajs_symbol_wrapper_drop`  | torajs-wrapper    |
//! | (other)         | `__torajs_rc_dec` + libc `free` | torajs-rc + libc  |
//!
//! `Closure` has no fixed extern: each closure env carries the
//! address of its Pass-2.5-synthesized `__env_drop_<closure>` at
//! [`CLOSURE_DROP_FN_OFF`] — the same indirect call the
//! lower-emitted typed drop makes (`emit_drop_closure`), rc-dec
//! gated here because the synthesized body walks + frees
//! unconditionally (typed sites own their single reference; this
//! dispatcher's contract is release-one-reference).
//!
//! Fallback (`Reserved6` / corrupt tags only — every live tag has an
//! arm): rc-dec; `free` on rc==0.
//!
//! `Response` is gated `#[cfg(not(target_os = "wasi"))]` (no libcurl
//! on WASI; mirrors runtime_str.c's `#ifndef __wasi__` gate).

use core::ffi::c_void;

use torajs_rc::{__torajs_rc_dec, ARR_KIND_HEAP, FLAG_ARR_ANY, FLAG_SUBCLASSED, HeapHeader, Tag};

/// Byte offset of the synthesized drop-fn pointer in a Closure env
/// block — ABI mirror of `torajs-core::ssa_lower::CLOSURE_DROP_FN_OFF`
/// (`{ hdr | fn_ptr@8 | drop_fn@16 | props@24 | boxed_entry@32 |
/// trace_fn@40 | caps@48+ }`, see the `Tag::Closure` doc in
/// torajs-rc).
const CLOSURE_DROP_FN_OFF: usize = 16;

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
    fn __torajs_arr_drop_any(p: *mut c_void);
    fn __torajs_arr_drop_heap(p: *mut c_void);
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    fn __torajs_cycle_buffer(p: *mut c_void);
    fn __torajs_bigint_drop(p: *mut c_void);
    fn __torajs_weakref_drop(p: *mut c_void);
    fn __torajs_weakmap_drop(p: *mut c_void);
    fn __torajs_weakset_drop(p: *mut c_void);
    fn __torajs_map_drop(p: *mut c_void);
    fn __torajs_set_drop(p: *mut c_void);
    fn __torajs_map_iter_drop(p: *mut c_void);
    fn __torajs_arr_iter_drop(p: *mut c_void);
    fn __torajs_iter_helper_drop(p: *mut c_void);
    fn __torajs_proxy_drop(p: *mut c_void);
    fn __torajs_arraybuffer_drop(p: *mut c_void);
    fn __torajs_dynobj_drop(p: *mut c_void);
    fn __torajs_accessor_drop(p: *mut c_void);
    fn __torajs_regex_drop(p: *mut c_void);
    fn __torajs_date_drop(p: *mut c_void);
    fn __torajs_symbol_drop(p: *mut c_void);
    fn __torajs_promise_drop(p: *mut c_void);
    fn __torajs_obj_drop_rc(p: *mut c_void);
    /// torajs-meta — scrub a dying exotic-subclass instance's
    /// identity entry (RFC 20260730 blade 0); every call is gated on
    /// `FLAG_SUBCLASSED`.
    fn __torajs_subclass_drop_entry(p: *mut c_void);
    // RFC 20260716-primitive-wrapper-substrate 刀 1 — three fixed
    // 16B wrapper blocks (torajs-wrapper). Each `_drop` is the direct
    // free path (matches the BigInt shape) called by the tag arm
    // AFTER `__torajs_rc_dec` returned 1.
    fn __torajs_number_wrapper_drop(p: *mut c_void);
    fn __torajs_string_wrapper_drop(p: *mut c_void);
    fn __torajs_boolean_wrapper_drop(p: *mut c_void);
    fn __torajs_symbol_wrapper_drop(p: *mut c_void);
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
        // RFC 20260704 S6 — pick the walker the block's own header
        // describes: Array<Any> walks its NaN-box slots; a typed
        // array marked ARR_KIND_HEAP at the typed→Any boundary walks
        // its cell-pointer slots; scalar kinds (I64/F64/BOOL) and
        // UNSET (never crossed into `any`) have no element refs to
        // release.
        t if t == Tag::Arr as u16 => unsafe {
            let flags = (*(child as *const HeapHeader)).flags;
            if flags & FLAG_ARR_ANY != 0 {
                __torajs_arr_drop_any(child);
            } else if (*(child as *const HeapHeader)).arr_elem_kind() == ARR_KIND_HEAP {
                __torajs_arr_drop_heap(child);
            } else {
                __torajs_arr_drop(child);
            }
        },
        #[cfg(not(target_os = "wasi"))]
        t if t == Tag::Response as u16 => unsafe { __torajs_response_drop(child) },
        t if t == Tag::BigInt as u16 => unsafe {
            // BigInt has no inner refs — rc-dec, then drop on hit-zero.
            if __torajs_rc_dec(child) != 0 {
                __torajs_bigint_drop(child);
            }
        },
        // RFC 20260716 刀 1 — primitive-wrapper arms. NumberWrapper /
        // BooleanWrapper are leaf blocks with no inner refs (rc-dec +
        // free, same shape as BigInt). StringWrapper carries one +1
        // reference on a Tag::Str cell; `__torajs_string_wrapper_drop`
        // routes that inner release through this very dispatcher (so
        // pool + tag walk stay uniform) before freeing the wrapper
        // block itself.
        t if t == Tag::NumberWrapper as u16 => unsafe {
            if __torajs_rc_dec(child) != 0 {
                __torajs_number_wrapper_drop(child);
            } else {
                // RFC 20260717 blade 3 — a wrapper that lost one of
                // several refs is a possible cycle root through its
                // expando props dict.
                __torajs_cycle_buffer(child);
            }
        },
        t if t == Tag::StringWrapper as u16 => unsafe {
            if __torajs_rc_dec(child) != 0 {
                __torajs_string_wrapper_drop(child);
            } else {
                __torajs_cycle_buffer(child);
            }
        },
        t if t == Tag::BooleanWrapper as u16 => unsafe {
            if __torajs_rc_dec(child) != 0 {
                __torajs_boolean_wrapper_drop(child);
            } else {
                __torajs_cycle_buffer(child);
            }
        },
        // SymbolWrapper mirrors StringWrapper — one +1 on a
        // Tag::Symbol cell released through this dispatcher.
        t if t == Tag::SymbolWrapper as u16 => unsafe {
            if __torajs_rc_dec(child) != 0 {
                __torajs_symbol_wrapper_drop(child);
            } else {
                __torajs_cycle_buffer(child);
            }
        },
        t if t == Tag::WeakRef as u16 => unsafe { __torajs_weakref_drop(child) },
        t if t == Tag::WeakMap as u16 => unsafe { __torajs_weakmap_drop(child) },
        t if t == Tag::WeakSet as u16 => unsafe { __torajs_weakset_drop(child) },
        t if t == Tag::Map as u16 => unsafe { __torajs_map_drop(child) },
        t if t == Tag::Set as u16 => unsafe { __torajs_set_drop(child) },
        t if t == Tag::MapIter as u16 => unsafe { __torajs_map_iter_drop(child) },
        t if t == Tag::ArrIter as u16 => unsafe { __torajs_arr_iter_drop(child) },
        t if t == Tag::IterHelper as u16 => unsafe { __torajs_iter_helper_drop(child) },
        t if t == Tag::Proxy as u16 => unsafe { __torajs_proxy_drop(child) },
        t if t == Tag::ArrayBuffer as u16 => unsafe { __torajs_arraybuffer_drop(child) },
        t if t == Tag::DynObj as u16 => unsafe { __torajs_dynobj_drop(child) },
        t if t == Tag::AccessorPair as u16 => unsafe { __torajs_accessor_drop(child) },
        t if t == Tag::RegExp as u16 => unsafe { __torajs_regex_drop(child) },
        t if t == Tag::Date as u16 => unsafe { __torajs_date_drop(child) },
        t if t == Tag::Symbol as u16 => unsafe { __torajs_symbol_drop(child) },
        t if t == Tag::Promise as u16 => unsafe { __torajs_promise_drop(child) },
        t if t == Tag::Obj as u16 => unsafe {
            // Class instance / anonymous struct — rc-aware layout
            // walk in torajs-cycle (reads the codegen-emitted
            // `__torajs_class_layouts` child offsets; mirrors the
            // typed path's cycle-root buffering for named classes).
            __torajs_obj_drop_rc(child)
        },
        t if t == Tag::Closure as u16 => unsafe {
            // Release one reference on the closure env cell. On
            // hit-zero, invoke the Pass-2.5-synthesized
            // `__env_drop_<closure>` whose address every construction
            // site stores at +16 (walks the props dynobj + Copy
            // capture boxes, then frees the env block) — the same
            // indirect call `emit_drop_closure` emits on the typed
            // path. The synthesized body has no rc gate of its own
            // (typed sites own their single reference), so the dec
            // gate lives here.
            if __torajs_rc_dec(child) != 0 {
                // RFC 20260730 blade 0 — a Function-subclass instance
                // (closure cell minted through a user class) scrubs
                // its identity entry before the env drop frees it.
                if (*(child as *const HeapHeader)).flags & FLAG_SUBCLASSED != 0 {
                    __torajs_subclass_drop_entry(child);
                }
                let drop_fn: unsafe extern "C" fn(*mut c_void) = core::mem::transmute(
                    (child.cast::<u8>().add(CLOSURE_DROP_FN_OFF) as *const usize).read(),
                );
                drop_fn(child);
            } else {
                // rc still positive — the exact cyclic-leak
                // condition (RFC 20260717 knife 3): buffer as a
                // PURPLE cycle candidate, mirroring the Obj and
                // wrapper arms. cycle_buffer's has_walkable_children
                // gate keeps untraceable envs (trace_fn 0, no
                // expando) zero-cost.
                __torajs_cycle_buffer(child);
            }
        },
        _ => unsafe {
            // Reserved6 / corrupt-tag fallback — rc-dec; on hit-zero
            // free the outer block. Every live tag has its own arm.
            if __torajs_rc_dec(child) != 0 {
                // Scrub from the cycle root buffer before the memory
                // goes away — a class Obj buffered as a cycle
                // candidate that then normal-drops to rc=0 here would
                // otherwise leave a dangling buffer entry (the
                // lower-emitted class drop path does the same scrub).
                // Cheap no-op when FLAG_BUFFERED is clear.
                __torajs_cycle_unbuffer(child);
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
