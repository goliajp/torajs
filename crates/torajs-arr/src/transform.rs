//! Array transformation ops — port of `runtime_str.c` L1278-1427.
//!
//! Six extern fns covering the `Array.prototype` transform family
//! that produces a NEW array (flat / concat) or mutates in place
//! (reverse / unshift / copyWithin / fill).
//!
//! All operate on 8-byte-slot `Array<T>` (NOT `Array<Any>` which
//! has 16-byte slots — those have their own dedicated paths in
//! [`crate::any`]). Element-type-agnostic at this level; the SSA
//! layer is responsible for type-correctness.
//!
//! T-13.5 deque-aware: `head_offset` is folded into per-slot
//! pointer math; in-place ops (reverse / fill) use the logical
//! slot 0 anchored at `arr_data(arr) + head * 8`. unshift reclaims
//! head>0 freed-front slots O(1) before falling back to the
//! grow / memmove path.

use core::ffi::c_void;

use crate::layout::{ARR_CAP_OFF, ARR_CELL_SIZE, ARR_DATA_PTR_OFF, ARR_LEN_OFF, TAG_ARR, arr_data};

/// `head_offset` lives in the high 32 bits of the u64 at
/// `ARR_CAP_OFF`. ops.rs uses the same constant for its
/// data-pointer math.
const ARR_HEAD_OFF: usize = ARR_CAP_OFF + 4;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(size: usize) -> *mut c_void;
}

#[inline]
pub(crate) unsafe fn arr_len(arr: *const u8) -> u64 {
    unsafe { (arr.add(ARR_LEN_OFF) as *const u64).read() }
}

#[inline]
pub(crate) unsafe fn set_arr_len(arr: *mut u8, v: u64) {
    unsafe { (arr.add(ARR_LEN_OFF) as *mut u64).write(v) };
}

#[inline]
pub(crate) unsafe fn arr_cap(arr: *const u8) -> u32 {
    unsafe { (arr.add(ARR_CAP_OFF) as *const u32).read() }
}

#[inline]
pub(crate) unsafe fn arr_head(arr: *const u8) -> u32 {
    unsafe { (arr.add(ARR_HEAD_OFF) as *const u32).read() }
}

#[inline]
pub(crate) unsafe fn set_arr_head(arr: *mut u8, v: u32) {
    unsafe { (arr.add(ARR_HEAD_OFF) as *mut u32).write(v) };
}

/// Pointer to logical slot 0 — folds `head_offset` into the math.
#[inline]
pub(crate) unsafe fn data_ptr(arr: *const u8) -> *mut u8 {
    let head = unsafe { arr_head(arr) } as usize;
    unsafe { arr_data(arr).add(head * 8) }
}

/// Pointer to physical slot `i` — bypasses `head_offset` so the
/// caller writes into the slack region (e.g. unshift's grow path).
#[inline]
pub(crate) unsafe fn data_ptr_raw(arr: *const u8, i: usize) -> *mut u8 {
    unsafe { arr_data(arr).add(i * 8) }
}

/// `arr_alloc_(len, cap)` mirror — fresh refcount=1 Array<T> block
/// with explicit len + cap. The public [`crate::__torajs_arr_alloc`]
/// sets len=0; this internal helper preserves the "alloc + write
/// len in one go" pattern from `runtime_str.c::arr_alloc_`.
pub(crate) unsafe fn arr_alloc_with(len: u64, cap: u64) -> *mut u8 {
    let block_size = ARR_CELL_SIZE + (cap as usize) * 8;
    let p = unsafe { malloc(block_size) } as *mut u8;
    if p.is_null() {
        torajs_abort::abort_with(b"OOM in Array alloc");
    }
    // Universal heap header: refcount u32 + type_tag u16 + flags u16.
    unsafe {
        (p as *mut u32).write(1);
        (p.add(4) as *mut u16).write(TAG_ARR);
        (p.add(6) as *mut u16).write(0);
        (p.add(ARR_LEN_OFF) as *mut u64).write(len);
        (p.add(ARR_CAP_OFF) as *mut u32).write(cap as u32);
        (p.add(ARR_HEAD_OFF) as *mut u32).write(0);
        // props NULL (chunk 516 slice fix, same latent-garbage class
        // — a props consumer's NULL gate would chase malloc garbage).
        (p.add(crate::layout::ARR_PROPS_OFF) as *mut u64).write(0);
        // B1 — data pointer starts self-referential (inline slots).
        (p.add(ARR_DATA_PTR_OFF) as *mut *mut u8).write(p.add(ARR_CELL_SIZE));
    }
    p
}

/// `arr.flat()` (depth-1) — single-level flatten of `Array<Array<T>>`
/// into a new `Array<T>`. Each inner slot's pointer is dereferenced
/// to read the inner array's slots, which get bulk-memcpy'd into the
/// result buffer. The copied slots are BORROWED: the lowering adopts
/// the product's range afterwards (`emit_adopt_copied_range`), since
/// only it knows whether `T` is refcounted.
///
/// §23.1.3.13.1 step 3.b skips an index HasProperty answers false for:
/// an inner array that can hold holes (`needs_index_mop`) is copied
/// index by index, holes left out, and the product's length is what
/// was copied.
///
/// # Safety
/// `outer` must be a valid Array<Array<T>> (slots hold valid Array
/// pointers, or NULL for a hole).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flat(outer: *const u8) -> *mut u8 {
    let outer_len = unsafe { arr_len(outer) };
    let mut total: u64 = 0;
    for i in 0..outer_len {
        let inner = unsafe { (data_ptr(outer).add(i as usize * 8) as *const *const u8).read() };
        // A hole of the outer cell is a NULL slot — absent to step 3.b.
        if inner.is_null() {
            continue;
        }
        total += unsafe { arr_len(inner) };
    }
    let p = unsafe { arr_alloc_with(total, total) };
    let mut cursor = 0usize;
    for i in 0..outer_len {
        let inner = unsafe { (data_ptr(outer).add(i as usize * 8) as *const *const u8).read() };
        if inner.is_null() {
            continue;
        }
        // Product slots come from the inner arrays — inherit their
        // element-descriptor bits (uniform T per the checker).
        unsafe { crate::layout::copy_elem_desc_bits(inner, p) };
        let inner_len = unsafe { arr_len(inner) } as usize;
        if unsafe { crate::layout::needs_index_mop(inner) } {
            for k in 0..inner_len as u64 {
                let f =
                    unsafe { crate::define_index_flags::__torajs_arr_index_flags(inner.cast(), k) };
                if f & crate::define::F_HOLE != 0 {
                    continue;
                }
                unsafe {
                    *(data_ptr_raw(p, 0).add(cursor) as *mut u64) =
                        *(data_ptr(inner).add(k as usize * 8) as *const u64)
                };
                cursor += 8;
            }
            continue;
        }
        if inner_len > 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data_ptr(inner),
                    data_ptr_raw(p, 0).add(cursor),
                    inner_len * 8,
                );
            }
            cursor += inner_len * 8;
        }
    }
    unsafe { (p.add(ARR_LEN_OFF) as *mut u64).write((cursor / 8) as u64) };
    p
}

/// `a.concat(b)` — fresh `[a..., b...]`. Element-type-agnostic
/// (8-byte slots). Single alloc + two memcpys. Subset is binary
/// only; multi-arg `a.concat(b, c, d)` deferred.
///
/// # Safety
/// Both inputs are valid 8-byte-slot Array<T> heap blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_concat(a: *const u8, b: *const u8) -> *mut u8 {
    let a_len = unsafe { arr_len(a) } as usize;
    let b_len = unsafe { arr_len(b) } as usize;
    let total = a_len + b_len;
    let p = unsafe { arr_alloc_with(total as u64, total as u64) };
    // Both inputs share one element type (checker invariant); an
    // empty `a` from an earlier derive still carries its bits.
    unsafe {
        crate::layout::copy_elem_desc_bits(a, p);
        crate::layout::copy_elem_desc_bits(b, p);
    }
    let dst = unsafe { data_ptr_raw(p, 0) };
    if a_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(data_ptr(a), dst, a_len * 8);
        }
    }
    if b_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(data_ptr(b), dst.add(a_len * 8), b_len * 8);
        }
    }
    p
}

/// `arr.reverse()` — in-place reverse over the 8-byte-slot array.
/// Returns the same pointer for chaining. Element-type-agnostic.
///
/// # Safety
/// `arr` must be a valid Array<T> heap block (8-byte slots).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_reverse(arr: *mut u8) -> *mut u8 {
    unsafe {
        let flags = (*(arr as *const torajs_rc::HeapHeader)).flags;
        if flags & torajs_rc::FLAG_ARR_ANY != 0 && crate::layout::needs_index_mop(arr) {
            return crate::reverse_mop::reverse_mop(arr);
        }
    }
    let len = unsafe { arr_len(arr) };
    if len < 2 {
        return arr;
    }
    // §10.1.9 [[Writable]] — a frozen receiver raises only when a slot
    // would actually be written: `Object.freeze([1]).reverse()` walks
    // zero pairs and must not throw (test262 array-has-one-entry).
    if unsafe { crate::define_length::__torajs_arr_frozen_guard(arr as *const core::ffi::c_void) }
        != 0
    {
        return arr;
    }
    let mut lo = 0u64;
    let mut hi = len - 1;
    while lo < hi {
        let a = unsafe { data_ptr(arr).add(lo as usize * 8) as *mut u64 };
        let b = unsafe { data_ptr(arr).add(hi as usize * 8) as *mut u64 };
        unsafe {
            let tmp = a.read();
            a.write(b.read());
            b.write(tmp);
        }
        lo += 1;
        hi -= 1;
    }
    if unsafe { crate::layout::needs_index_mop(arr) } {
        let last = len - 1;
        unsafe { crate::define_hole::move_holes(arr.cast(), len, len, |i| Some(last - i)) };
    }
    arr
}

/// `arr.unshift(v)` — insert `v` at logical slot[0]. O(1) when
/// `head_offset > 0` (reclaim a freed front slot); falls back to
/// grow / memmove when head==0. Returns the (possibly realloc'd)
/// pointer, mirroring `push`'s contract.
///
/// # Safety
/// `arr` must be a valid Array<T> heap block (8-byte slots, T = i64
/// in the SSA layer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_unshift(arr: *mut u8, v: i64) -> *mut u8 {
    let old_len = unsafe { arr_len(arr) };
    unsafe { unshift_slot(arr, v) };
    if unsafe { arr_len(arr) } > old_len && unsafe { crate::layout::needs_index_mop(arr) } {
        unsafe {
            crate::define_hole::move_holes(arr.cast(), old_len, old_len + 1, |i| Some(i + 1))
        };
    }
    arr
}

/// The slot half of [`__torajs_arr_unshift`]: guard, then store `v`
/// at logical slot 0.
unsafe fn unshift_slot(arr: *mut u8, v: i64) -> *mut u8 {
    // A new index on a non-extensible receiver is a TypeError
    // (`__torajs_arr_extend_guard`). Nothing to release here, and that
    // is deliberate: on this lane the raw slot value is BORROWED at
    // call time — the lowering incs the array's stake only after the
    // call returns — so a kernel-side drop would release a stake the
    // array never received. The lowering's throw check right after
    // the call (`emit_grow_kernel_checked`) is what settles the temp.
    if unsafe { crate::define_length::__torajs_arr_extend_guard(arr as *const core::ffi::c_void) }
        != 0
    {
        return arr;
    }
    let head = unsafe { arr_head(arr) };
    if head > 0 {
        // Fast path: reclaim a freed front slot.
        let new_head = head - 1;
        unsafe { set_arr_head(arr, new_head) };
        unsafe {
            (data_ptr_raw(arr, new_head as usize) as *mut i64).write(v);
        }
        unsafe { set_arr_len(arr, arr_len(arr) + 1) };
        return arr;
    }
    let len = unsafe { arr_len(arr) } as usize;
    let cap = unsafe { arr_cap(arr) } as usize;
    if len >= cap {
        // Grow the data buffer (cell never moves — B1), then shift
        // the live range right one slot so logical slot 0 lands at
        // physical 0 for the new value.
        let new_cap = if cap == 0 { 1 } else { cap * 2 };
        unsafe { crate::grow::grow_data_buffer(arr, new_cap as u64) };
        if len > 0 {
            unsafe {
                core::ptr::copy(data_ptr(arr), data_ptr_raw(arr, 1), len * 8);
            }
        }
        unsafe {
            (data_ptr_raw(arr, 0) as *mut i64).write(v);
            set_arr_len(arr, (len + 1) as u64);
        }
        return arr;
    }
    // In-place: head==0 + cap room → memmove right and prepend.
    if len > 0 {
        unsafe {
            core::ptr::copy(data_ptr(arr), data_ptr_raw(arr, 1), len * 8);
        }
    }
    unsafe {
        (data_ptr_raw(arr, 0) as *mut i64).write(v);
        set_arr_len(arr, (len + 1) as u64);
    }
    arr
}

#[inline]
fn clamp(i: i64, lo: i64, hi: i64) -> i64 {
    if i < lo {
        lo
    } else if i > hi {
        hi
    } else {
        i
    }
}

/// `arr.copyWithin(target, start, end)` — in-place memmove of
/// `[start, end)` to position `target`. All indices clamped to
/// `[0, len]`. memmove handles overlap.
///
/// # Safety
/// `arr` must be a valid Array<T> heap block (8-byte slots).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_copy_within(
    arr: *mut u8,
    target: i64,
    start: i64,
    end: i64,
) -> *mut u8 {
    let len = unsafe { arr_len(arr) } as i64;
    let lo = clamp(start, 0, len);
    let hi = clamp(end, 0, len);
    let to = clamp(target, 0, len);
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
    // frozen → TypeError only once a slot is about to be written
    // (count > 0 here); a zero-count call on a frozen array is legal.
    if unsafe { crate::define_length::__torajs_arr_frozen_guard(arr as *const core::ffi::c_void) }
        != 0
    {
        return arr;
    }
    unsafe {
        core::ptr::copy(
            data_ptr(arr).add(lo as usize * 8),
            data_ptr(arr).add(to as usize * 8),
            count as usize * 8,
        );
    }
    if unsafe { crate::layout::needs_index_mop(arr) } {
        let (lo, to, count) = (lo as u64, to as u64, count as u64);
        for h in unsafe { crate::define_hole::take_holes(arr.cast(), 0, len as u64) } {
            // A hole outside the target keeps its place; one in the
            // source range also lands at its copy (§23.1.3.4 step
            // 16.c DeletePropertyOrThrow).
            if !(to..to + count).contains(&h) {
                unsafe { crate::define_hole::mark_hole_range(arr.cast(), h, h + 1) };
            }
            if (lo..lo + count).contains(&h) {
                let n = h - lo + to;
                unsafe { crate::define_hole::mark_hole_range(arr.cast(), n, n + 1) };
            }
        }
    }
    arr
}

// `__torajs_arr_splice` + the `...items` insert sibling live in
// `transform_splice.rs` (RFC 20260720-splice-insert knife 1 — this
// file sat at 474 LOC and the insert kernel would cross the 500
// hard limit).

/// `arr.fill(value, start, end)` — write `value` into `[start, end)`.
/// Indices clamped to `[0, len]`. Element-type-agnostic — value is
/// passed as i64 and stored verbatim; the SSA layer handles type
/// conversion.
///
/// # Safety
/// `arr` must be a valid Array<T> heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_fill(
    arr: *mut u8,
    value: i64,
    start: i64,
    end: i64,
) -> *mut u8 {
    let len = unsafe { arr_len(arr) } as i64;
    let lo = clamp(start, 0, len);
    let hi = clamp(end, 0, len);
    if hi < lo {
        return arr;
    }
    // frozen → TypeError only for a non-empty range: `Object.freeze([])
    // .fill(1)` sets nothing and must not throw (test262
    // return-abrupt-from-setting-property-value freezes an empty array
    // first). `value` is a Copy raw slot on this lane.
    if lo < hi
        && unsafe {
            crate::define_length::__torajs_arr_frozen_guard(arr as *const core::ffi::c_void)
        } != 0
    {
        return arr;
    }
    for i in lo..hi {
        unsafe { (data_ptr(arr).add(i as usize * 8) as *mut i64).write(value) };
    }
    if lo < hi && unsafe { crate::layout::needs_index_mop(arr) } {
        // A filled index is present again; the rest keep their marks.
        let (lo, hi, len) = (lo as u64, hi as u64, len as u64);
        let keep = |i| (i < lo || i >= hi).then_some(i);
        unsafe { crate::define_hole::move_holes(arr.cast(), len, len, keep) };
    }
    arr
}
