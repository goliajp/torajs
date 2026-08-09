//! Any-receiver transform kernels — concat / copyWithin / splice
//! (any-method dispatch backfill chunk 2; fill rides the existing
//! [`crate::any::__torajs_arr_fill_any`]).
//!
//! Every entry is kind-aware over the block's own self-description,
//! same dispatch as [`crate::slice::__torajs_arr_any_slice`]:
//! `FLAG_ARR_ANY` (NaN-box u64 slots) vs typed 8-byte slots reached
//! through a static `Arr<Any>` view (elem kind recorded at the
//! coercion boundary). Slot strides match at 8 bytes, so the move /
//! copy machinery is shared; only the rc ledger and the insert
//! coercion differ per shape.

use core::ffi::c_void;

use torajs_rc::{ARR_ELEM_KIND_MASK, ARR_KIND_HEAP, FLAG_ARR_ANY, HeapHeader};

use crate::any::{ANY_HEAP, ANY_UNDEF};
use crate::any_typed_bridge::{coerce_raw_scalar, kind_to_any_tag};
use crate::grow::grow_data_buffer;
use crate::layout::{ARR_LEN_OFF, TAG_ARR, arr_data};

/// T-13.5 packed cap (u32) + head_offset (u32) at offset 16 —
/// mirrors `transform.rs` / `slice.rs`.
const ARR_CAP_LOW32_OFF: usize = 16;
const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    /// Cross-tier — torajs-rc. NaN-box-safe refcount bump (no-ops
    /// for non-cell bit patterns and NULL).
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — universal heap dropper (NaN-box-safe).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-anyvalue NaN-box unpack.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Cross-tier — torajs-throw catchable TypeError (records via
    /// TLS; the caller's emit_throw_check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

#[inline]
unsafe fn arr_len(arr: *const u8) -> i64 {
    unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) as i64 }
}

#[inline]
unsafe fn arr_head(arr: *const u8) -> u32 {
    unsafe { *(arr.add(ARR_HEAD_OFF) as *const u32) }
}

/// Pointer to logical slot `i` (deque head folded).
#[inline]
unsafe fn slot_ptr(arr: *const u8, i: i64) -> *mut u64 {
    unsafe { arr_data(arr).add((arr_head(arr) as usize + i as usize) * 8) as *mut u64 }
}

/// ES §23.1.3 relative-index normalize shared by copyWithin:
/// negative wraps from the end, then clamp to `[0, len]`.
#[inline]
fn wrap_clamp(v: i64, len: i64) -> i64 {
    let w = if v < 0 { v + len } else { v };
    w.clamp(0, len)
}

/// `xs.concat(...items)` for an any receiver per ES §23.1.3.1
/// (subset: no `@@isConcatSpreadable` surface — an `Tag::Arr` heap
/// argument spreads, everything else appends as one element).
///
/// The seed is a kind-aware fresh copy of the receiver
/// (`arr_any_slice`, both shapes) — the same idiom as the typed-tier
/// `Arr<Any>` concat lowering; `extend_any` / `push_any` then handle
/// per-shape coercion and the per-cell rc ledger.
///
/// A typed-kind seed transitions to `FLAG_ARR_ANY` before use (the
/// slice product is fresh, rc=1, unaliased — the exact legality
/// window `transition_fresh_to_any` documents): the product lives in
/// the any world where spec arrays are heterogeneous, and a raw-kind
/// product raised the element-kind guard on a later string write
/// (`Array.prototype.concat.call([101])` then `p["0"] = "s"` — the
/// rotation-176 sweep regression this closes).
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `argv` holds `argc`
/// BORROWED NaN-box AnyValues. Returned pointer is fresh (+1 rc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_concat(
    arr: *const u8,
    argv: *const u64,
    argc: i64,
) -> *mut u8 {
    unsafe {
        let dst = crate::slice::__torajs_arr_any_slice(arr, 0, i64::MAX);
        if (*(dst as *const torajs_rc::HeapHeader)).flags & torajs_rc::FLAG_ARR_ANY == 0 {
            crate::any_typed_bridge::transition_fresh_to_any(dst);
        }
        concat_extend_args(dst, argv, argc)
    }
}

/// The §23.1.3.1 per-argument loop shared by both concat entries:
/// a `Tag::Arr` heap argument spreads, everything else appends as
/// one element.
unsafe fn concat_extend_args(mut dst: *mut u8, argv: *const u64, argc: i64) -> *mut u8 {
    unsafe {
        for k in 0..argc {
            let av = *argv.add(k as usize);
            let tag = __torajs_anyv_unbox_tag(av) as u64;
            if tag == ANY_HEAP {
                let inner = __torajs_anyv_unbox_value(av) as *const u8;
                if !inner.is_null() && *(inner.add(4) as *const u16) == TAG_ARR {
                    dst = crate::any::__torajs_arr_extend_any(dst, inner);
                    continue;
                }
            }
            // Non-array argument appends as one element. push_any
            // takes ownership of heap cells; argv is borrowed → +1
            // first (mirrors flat_any's carry-through).
            __torajs_rc_inc(av as *mut c_void);
            dst = crate::any::__torajs_arr_push_any(
                dst as *mut c_void,
                tag,
                __torajs_anyv_unbox_value(av) as u64,
            );
        }
        dst
    }
}

/// `Array.prototype.concat.call(obj, ...items)` for a NON-array
/// object receiver (RFC 20260721 刀 8-B) — §23.1.3.1 steps 1-2:
/// `ArraySpeciesCreate(O, 0)` answers a plain `ArrayCreate` because
/// `IsArray(O)` is false (the receiver's `constructor` is never
/// consulted), then O itself appends as the single seed element
/// (not spreadable — no `@@isConcatSpreadable` surface, so only
/// real Arrays spread). `Get(O, "length")` never runs — concat has
/// no length read, an accessor `length` must not fire.
///
/// # Safety
/// `recv` is a BORROWED NaN-box AnyValue holding a heap cell;
/// `argv` holds `argc` BORROWED NaN-box AnyValues. Returned pointer
/// is fresh (+1 rc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_concat_generic(
    recv: u64,
    argv: *const u64,
    argc: i64,
) -> *mut u8 {
    unsafe {
        let mut dst = crate::__torajs_arr_alloc_any(argc as u64 + 1);
        // The seed slot takes its own stake — recv is borrowed.
        __torajs_rc_inc(recv as *mut c_void);
        dst = crate::any::__torajs_arr_push_any(
            dst as *mut c_void,
            __torajs_anyv_unbox_tag(recv) as u64,
            __torajs_anyv_unbox_value(recv) as u64,
        );
        concat_extend_args(dst, argv, argc)
    }
}

/// `xs.copyWithin(target, start, end)` for an any receiver per ES
/// §23.1.3.4 — raw relative indices (the kernel wraps + clamps).
/// In-place move; answers the same pointer for chaining.
///
/// rc ledger for slots that carry references (`FLAG_ARR_ANY`
/// NaN-box cells / typed `ARR_KIND_HEAP` raw pointers): inc the
/// source range first, then drop the overwritten destination range,
/// then memmove — the source stake survives the overlap window
/// (same order as the typed-tier SSA `emit_copy_within_rc_ranges`).
/// Scalar / UNSET kinds are a plain memmove.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_copy_within(
    arr: *mut u8,
    target: i64,
    start: i64,
    end: i64,
) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — the block move crosses the
        // unmaterialized tail; loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.copyWithin\0".as_ptr(),
        ) {
            return arr;
        }
        let len = arr_len(arr);
        let to = wrap_clamp(target, len);
        let lo = wrap_clamp(start, len);
        let hi = wrap_clamp(end, len);
        if hi <= lo {
            return arr;
        }
        let mut count = hi - lo;
        if to + count > len {
            count = len - to;
            if count <= 0 {
                return arr;
            }
        }
        let header = &*(arr as *const HeapHeader);
        let needs_rc = header.flags & FLAG_ARR_ANY != 0 || header.arr_elem_kind() == ARR_KIND_HEAP;
        if needs_rc {
            for i in 0..count {
                __torajs_rc_inc((*slot_ptr(arr, lo + i)) as *mut c_void);
            }
            for i in 0..count {
                __torajs_value_drop_heap((*slot_ptr(arr, to + i)) as *mut c_void);
            }
        }
        core::ptr::copy(
            slot_ptr(arr, lo) as *const u8,
            slot_ptr(arr, to) as *mut u8,
            count as usize * 8,
        );
        arr
    }
}

/// `xs.splice(start, deleteCount, ...items)` for an any receiver per
/// ES §23.1.3.31. `actual_start` / `actual_delete` arrive already
/// normalized (the dispatch arm owns the argc-dependent §5-7 steps);
/// `items` are BORROWED NaN-box AnyValues.
///
/// Removed slots TRANSFER their references into the returned array
/// (net-zero rc); inserted slots take a fresh stake per shape:
/// NaN-box cells rc_inc as-is, typed HEAP slots unwrap the pointer
/// (undefined holes to NULL), typed scalar slots kind-coerce — any
/// non-storable item raises a catchable TypeError BEFORE the
/// receiver is touched (pre-scan, no partial mutation).
///
/// The receiver cell never moves (B1 — grow swaps the data buffer),
/// so no write-back slot is needed.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer;
/// `0 <= actual_start <= len`,
/// `0 <= actual_delete <= len - actual_start`. Returned pointer is
/// the fresh (+1 rc) removed array.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_splice(
    arr: *mut u8,
    actual_start: i64,
    actual_delete: i64,
    items: *const u64,
    item_count: i64,
) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — the relocation walk crosses the
        // unmaterialized tail; loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.splice\0".as_ptr(),
        ) {
            return crate::alloc::__torajs_arr_alloc_any(0);
        }
        let len = arr_len(arr);
        let header = &*(arr as *const HeapHeader);
        let is_any = header.flags & FLAG_ARR_ANY != 0;
        let kind = header.arr_elem_kind();

        // 刀 6 G9a — frozen rejects before any mutation; a net-growth
        // shape with a locked length rejects early too (§10.4.2.1
        // step 2.c). The shrink shape mutates first and throws at the
        // step-24 length write (`splice_finish_len`), spec order.
        if header.flags & torajs_rc::FLAG_FROZEN != 0
            || (item_count > actual_delete && header.flags & torajs_rc::FLAG_ARR_LENGTH_RO != 0)
        {
            crate::define_length::__torajs_arr_len_write_guard(arr as *const _);
            return crate::alloc::__torajs_arr_alloc_any(0);
        }

        // Typed pre-scan: every item must be storable without
        // changing the elem kind (loud TypeError, receiver + rc
        // ledger untouched — same admit as typed_push_pair).
        if !is_any {
            if kind_to_any_tag(kind).is_none() {
                __torajs_throw_type_error(
                    c"array of unknown element kind reached an any[] splice".as_ptr(),
                );
                return crate::alloc::__torajs_arr_alloc_any(0);
            }
            for k in 0..item_count {
                let av = *items.add(k as usize);
                let tag = __torajs_anyv_unbox_tag(av) as u64;
                let ok = if kind == ARR_KIND_HEAP {
                    tag == ANY_HEAP || tag == ANY_UNDEF
                } else {
                    coerce_raw_scalar(kind, tag, __torajs_anyv_unbox_value(av) as u64).is_some()
                };
                if !ok {
                    __torajs_throw_type_error(
                        c"splice through an any[] view would change the typed array's element kind"
                            .as_ptr(),
                    );
                    return crate::alloc::__torajs_arr_alloc_any(0);
                }
            }
        }

        // Fold a deque head down to 0 so the gap math below works on
        // physical slot 0 (typed receivers can have shifted).
        let head = arr_head(arr);
        if head > 0 {
            core::ptr::copy(
                arr_data(arr).add(head as usize * 8),
                arr_data(arr),
                len as usize * 8,
            );
            *(arr.add(ARR_HEAD_OFF) as *mut u32) = 0;
        }

        // Removed array — slots transfer (no rc traffic), header
        // flags follow the receiver's shape.
        let removed = if is_any {
            let p = crate::alloc::__torajs_arr_alloc_any(actual_delete as u64);
            *(p.add(ARR_LEN_OFF) as *mut u64) = actual_delete as u64;
            p
        } else {
            let p = crate::slice::arr_alloc_fresh(actual_delete as u64, actual_delete as u64);
            *(p.add(6) as *mut u16) |= header.flags & ARR_ELEM_KIND_MASK;
            p
        };
        if actual_delete > 0 {
            core::ptr::copy_nonoverlapping(
                arr_data(arr).add(actual_start as usize * 8),
                arr_data(removed),
                actual_delete as usize * 8,
            );
        }

        let diff = item_count - actual_delete;
        let new_len = len + diff;
        if diff > 0 {
            let cap = *(arr.add(ARR_CAP_LOW32_OFF) as *const u32) as i64;
            if new_len > cap {
                let grown = if cap * 2 > new_len { cap * 2 } else { new_len };
                grow_data_buffer(arr, grown as u64);
            }
        }
        let trailing = len - actual_start - actual_delete;
        if trailing > 0 && diff != 0 {
            core::ptr::copy(
                arr_data(arr).add((actual_start + actual_delete) as usize * 8),
                arr_data(arr).add((actual_start + item_count) as usize * 8),
                trailing as usize * 8,
            );
        }
        for k in 0..item_count {
            let av = *items.add(k as usize);
            let slot = arr_data(arr).add((actual_start + k) as usize * 8) as *mut u64;
            if is_any {
                __torajs_rc_inc(av as *mut c_void);
                *slot = av;
            } else if kind == ARR_KIND_HEAP {
                let tag = __torajs_anyv_unbox_tag(av) as u64;
                let raw = if tag == ANY_UNDEF {
                    0
                } else {
                    __torajs_anyv_unbox_value(av) as u64
                };
                __torajs_rc_inc(raw as *mut c_void);
                *slot = raw;
            } else {
                // Pre-scan guarantees Some — coerce is pure.
                *slot = coerce_raw_scalar(
                    kind,
                    __torajs_anyv_unbox_tag(av) as u64,
                    __torajs_anyv_unbox_value(av) as u64,
                )
                .unwrap();
            }
        }
        crate::transform_splice::splice_finish_len(arr, len as u64, new_len as u64);
        crate::define_hole::splice_remap_holes(
            arr as *mut core::ffi::c_void,
            actual_start,
            actual_delete,
            item_count,
            len as i64,
        );
        removed
    }
}
