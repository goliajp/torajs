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
unsafe fn arr_len(arr: *const u8) -> u64 {
    unsafe { (arr.add(ARR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn set_arr_len(arr: *mut u8, v: u64) {
    unsafe { (arr.add(ARR_LEN_OFF) as *mut u64).write(v) };
}

#[inline]
unsafe fn arr_cap(arr: *const u8) -> u32 {
    unsafe { (arr.add(ARR_CAP_OFF) as *const u32).read() }
}

#[inline]
unsafe fn arr_head(arr: *const u8) -> u32 {
    unsafe { (arr.add(ARR_HEAD_OFF) as *const u32).read() }
}

#[inline]
unsafe fn set_arr_head(arr: *mut u8, v: u32) {
    unsafe { (arr.add(ARR_HEAD_OFF) as *mut u32).write(v) };
}

/// Pointer to logical slot 0 — folds `head_offset` into the math.
#[inline]
unsafe fn data_ptr(arr: *const u8) -> *mut u8 {
    let head = unsafe { arr_head(arr) } as usize;
    unsafe { arr_data(arr).add(head * 8) }
}

/// Pointer to physical slot `i` — bypasses `head_offset` so the
/// caller writes into the slack region (e.g. unshift's grow path).
#[inline]
unsafe fn data_ptr_raw(arr: *const u8, i: usize) -> *mut u8 {
    unsafe { arr_data(arr).add(i * 8) }
}

/// `arr_alloc_(len, cap)` mirror — fresh refcount=1 Array<T> block
/// with explicit len + cap. The public [`crate::__torajs_arr_alloc`]
/// sets len=0; this internal helper preserves the "alloc + write
/// len in one go" pattern from `runtime_str.c::arr_alloc_`.
unsafe fn arr_alloc_with(len: u64, cap: u64) -> *mut u8 {
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
/// result buffer.
///
/// # Safety
/// `outer` must be a valid Array<Array<T>> (slots hold valid Array
/// pointers).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flat(outer: *const u8) -> *mut u8 {
    let outer_len = unsafe { arr_len(outer) };
    let mut total: u64 = 0;
    for i in 0..outer_len {
        let inner = unsafe { (data_ptr(outer).add(i as usize * 8) as *const *const u8).read() };
        total += unsafe { arr_len(inner) };
    }
    let p = unsafe { arr_alloc_with(total, total) };
    let mut cursor = 0usize;
    for i in 0..outer_len {
        let inner = unsafe { (data_ptr(outer).add(i as usize * 8) as *const *const u8).read() };
        let inner_len = unsafe { arr_len(inner) } as usize;
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
    let len = unsafe { arr_len(arr) };
    if len < 2 {
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
    unsafe {
        core::ptr::copy(
            data_ptr(arr).add(lo as usize * 8),
            data_ptr(arr).add(to as usize * 8),
            count as usize * 8,
        );
    }
    arr
}

/// `arr.splice(start, delete_count)` — remove `delete_count` slots
/// starting at logical `start`, returning the removed slice as a
/// fresh `Array<T>`. Trailing slots compact left into the gap; the
/// receiver's `len` shrinks by the actual delete count. Subset:
/// no `...items` insert args (ES rest-arg surface deferred).
///
/// Per ES spec §23.1.3.31:
///   - `start < 0`        → `max(len + start, 0)`
///   - `start > len`      → `len`
///   - `delete_count < 0` → `0`
///   - `delete_count > len - actual_start` → `len - actual_start`
///
/// Receiver pointer is unchanged (no realloc — splice only shrinks
/// the live range), so the SSA dispatch can skip the slot-writeback
/// that push / unshift need.
///
/// # Safety
/// `arr` must be a valid Array<T> heap block (8-byte slots).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_splice(
    arr: *mut u8,
    start: i64,
    delete_count: i64,
) -> *mut u8 {
    let len = unsafe { arr_len(arr) } as i64;
    let actual_start = if start < 0 {
        let s = start + len;
        if s < 0 { 0 } else { s }
    } else if start > len {
        len
    } else {
        start
    };
    let actual_delete = if delete_count < 0 {
        0
    } else if delete_count > len - actual_start {
        len - actual_start
    } else {
        delete_count
    };
    let removed = unsafe { arr_alloc_with(actual_delete as u64, actual_delete as u64) };
    if actual_delete > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                data_ptr(arr).add(actual_start as usize * 8),
                data_ptr_raw(removed, 0),
                actual_delete as usize * 8,
            );
        }
    }
    let trailing = len - actual_start - actual_delete;
    if trailing > 0 && actual_delete > 0 {
        unsafe {
            core::ptr::copy(
                data_ptr(arr).add((actual_start + actual_delete) as usize * 8),
                data_ptr(arr).add(actual_start as usize * 8),
                trailing as usize * 8,
            );
        }
    }
    unsafe {
        set_arr_len(arr, (len - actual_delete) as u64);
    }
    removed
}

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
    for i in lo..hi {
        unsafe { (data_ptr(arr).add(i as usize * 8) as *mut i64).write(value) };
    }
    arr
}

// ============================================================
// ES2023 non-mutating toReversed / with — moved from `join.rs`
// (chunk 624 file-size split; they are transforms, not joins).
// ============================================================

unsafe extern "C" {
    /// Cross-tier — torajs-throw catchable RangeError (records via
    /// TLS; the caller's emit_throw_check propagates).
    fn __torajs_throw_range_error(msg: *const u8);
}

#[inline]
unsafe fn slot_addr(arr: *const u8, i: u64) -> *const u8 {
    unsafe { arr_data(arr).add((arr_head(arr) as usize + i as usize) * 8) }
}

/// Internal alloc for `Array<T>` (matches C's `arr_alloc_`).
/// Bypasses cap-matched pool — to_reversed/with always produce a fresh
/// right-sized block.
#[inline]
unsafe fn arr_alloc_fresh(len: u64, cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_CELL_SIZE + (cap as usize) * 8;
        let p = malloc(total) as *mut u8;
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = 0;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_OFF) as *mut u32) = cap as u32;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
        // props NULL (chunk 516 slice fix, same latent-garbage class)
        // + B1 self-referential data pointer.
        *(p.add(crate::layout::ARR_PROPS_OFF) as *mut u64) = 0;
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
        p
    }
}

// ============================================================
// arr_to_reversed — ES2023 non-mutating reverse
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_to_reversed(arr: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let p = arr_alloc_fresh(len, len);
        let dst = arr_data(p);
        for i in 0..len {
            let src = slot_addr(arr, len - 1 - i);
            *(dst.add(i as usize * 8) as *mut u64) = *(src as *const u64);
        }
        p
    }
}

// ============================================================
// arr_with — ES2023 non-mutating index update
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_with(arr: *const u8, i: i64, v: i64) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        // ES §23.1.3.39 step 7 — `actualIndex = i < 0 ? len + i : i`,
        // throw RangeError when out-of-range. Pre-fix this fell
        // through to the unchecked slot store and silently corrupted
        // adjacent heap (or wrote past the alloc on small caps).
        let adj = if i < 0 { len as i64 + i } else { i };
        if adj < 0 || adj >= len as i64 {
            __torajs_throw_range_error(b"Array index out of range\0".as_ptr());
            return core::ptr::null_mut();
        }
        let p = arr_alloc_fresh(len, len);
        let dst = arr_data(p);
        if len > 0 {
            let src = slot_addr(arr, 0);
            core::ptr::copy_nonoverlapping(src, dst, (len as usize) * 8);
        }
        *(dst.add(adj as usize * 8) as *mut u64) = v as u64;
        p
    }
}
