//! Array heap-block layout constants.
//!
//! RFC 20260706-arr-grow-alias-stability B1 (2026-07-06): the cell is
//! FIXED — slots moved behind a data pointer so grow never relocates
//! the cell (V8 `elements` / JSC butterfly shape, SmallVector-style
//! inline-then-spill):
//!
//! ```text
//! offset | size | field
//! -------|------|------
//!   0    |  8B  | universal heap header (refcount + type_tag + flags)
//!   8    |  8B  | len (current element count, u64)
//!  16    |  8B  | cap (u32) + head_offset (u32) — packed T-13.5;
//!                  cap describes the CURRENT data buffer
//!  24    |  8B  | props_dynobj — pointer to inline arr-prop dynobj or
//!                  NULL when no `arr.x = v` was ever written
//!  32    |  8B  | data — pointer to slots[cap]. Freshly allocated
//!                  arrays point it at the inline region (cell+40,
//!                  same malloc block — zero locality change); grow
//!                  swaps it to an independent buffer and the cell
//!                  never moves. Aliases stay valid across grow.
//!  40    | 8×n  | inline slots region (only while data == cell+40;
//!                  dead space after a grow spilled to a buffer)
//! ```
//!
//! Sub-types (encoded via the universal header's `type_tag` /
//! `flags`):
//! - `Array<T>` (T is a tightly-typed scalar) → slots are raw values
//! - `Array<Any>` (FLAG_ARR_ANY set) → each slot is an 8-byte NaN-box
//! - `Array<Substr>` split-block (FLAG_SPLIT_BLOCK in `torajs-str`) —
//!   same 40-byte cell (B1 sync); split blocks never grow so their
//!   data pointer is permanently self-referential. See
//!   `torajs-str/src/split/pool.rs::ARR_HDR_SIZE`.

/// `type_tag` value for Array heap blocks (matches
/// `runtime_str.c::__TORAJS_TAG_ARR`).
pub const TAG_ARR: u16 = 2;

/// Universal heap header size (`{ refcount: u32, type_tag: u16, flags: u16 }`).
pub const HEAP_HEADER_SIZE: usize = 8;

/// Offset of the `len` u64 within the heap block.
pub const ARR_LEN_OFF: usize = HEAP_HEADER_SIZE;

/// Offset of the `cap` u64 within the heap block.
pub const ARR_CAP_OFF: usize = HEAP_HEADER_SIZE + 8;

/// Offset of the inline `props_dynobj` slot (Round 4 chunk 5a).
/// `*mut DynObj` or NULL; set by `__torajs_arrprops_set` (chunk 5b+),
/// read by `__torajs_arr_drop` to release the chained props block.
pub const ARR_PROPS_OFF: usize = HEAP_HEADER_SIZE + 16;

/// Offset of the `data` pointer field — points at `slots[0]`
/// (RFC 20260706-arr-grow-alias-stability B1). Cross-crate sync sites
/// (must move in lockstep):
/// - `torajs_core::ssa_lower::ARR_DATA_PTR_OFF` (IR emit)
/// - `torajs_cycle::layout` (cycle walker mirror)
/// - `torajs_str::split::pool` (split-block layout)
pub const ARR_DATA_PTR_OFF: usize = HEAP_HEADER_SIZE + 24;

/// Fixed cell size — header through the `data` pointer field. The
/// inline slots region starts here; `arr_data(cell)` == `cell +
/// ARR_CELL_SIZE` exactly while the array has never grown.
pub const ARR_CELL_SIZE: usize = HEAP_HEADER_SIZE + 32;

/// Read the slots base pointer from a cell.
///
/// # Safety
/// `arr` must be a live array cell with an initialized data field.
#[inline]
pub unsafe fn arr_data(arr: *const u8) -> *mut u8 {
    unsafe { *(arr.add(ARR_DATA_PTR_OFF) as *const *mut u8) }
}

/// True when the data pointer still targets the cell's own inline
/// region (single-block state; grow spills to an external buffer).
///
/// # Safety
/// Same contract as [`arr_data`].
#[inline]
pub unsafe fn arr_data_is_inline(arr: *const u8) -> bool {
    unsafe { arr_data(arr) == (arr as *mut u8).add(ARR_CELL_SIZE) }
}

/// The number of slots that physically exist in the data buffer —
/// `len`, unless the cell carries a sparse tail
/// (`FLAG_ARR_SPARSE_TAIL`), whose invariant is `materialized extent
/// == cap` (the sparse transition compacts the deque head and trims
/// the buffer to the old length, so `cap` doubles as the extent —
/// no new header field). Every walk that touches raw slots (drop,
/// teardown, shrink, enumeration) must use this, not `len`: indexes
/// in `[extent, len)` are implicit holes with no storage (RFC
/// 20260810-arr-sparse-grow).
///
/// # Safety
/// `arr` must be a live array cell.
#[inline]
pub unsafe fn arr_live_extent(arr: *const u8) -> u64 {
    unsafe {
        let flags = *(arr.add(6) as *const u16);
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        if flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0 {
            let cap = *(arr.add(ARR_CAP_OFF) as *const u32) as u64;
            if cap < len { cap } else { len }
        } else {
            len
        }
    }
}

/// Copy the element-descriptor bits (`FLAG_ARR_ANY` +
/// `ARR_ELEM_KIND_*`) from `src`'s header onto the fresh derive
/// product `dst`. Every clone that memcpys slots verbatim (slice /
/// toReversed / with / splice-removed / concat / flat) must stay
/// self-describing: an `Array<Any>` source's NaN-box slots read back
/// through `arr_get_any_tag`, whose `ARR_KIND_UNSET` contract answers
/// `undefined` — a flags=0 clone silently blanks every element (RFC
/// 20260713-array-proto-residual B1). Mirrors `arr_any_slice`'s
/// kind-bits propagation; mutability faces (FLAG_FROZEN /
/// FLAG_ARR_LENGTH_RO / exotic-index) deliberately stay behind —
/// derive products are fresh ordinary arrays per spec.
///
/// # Safety
/// Both pointers are valid array cells with initialized headers.
#[inline]
pub(crate) unsafe fn copy_elem_desc_bits(src: *const u8, dst: *mut u8) {
    unsafe {
        let bits =
            *(src.add(6) as *const u16) & (torajs_rc::FLAG_ARR_ANY | torajs_rc::ARR_ELEM_KIND_MASK);
        *(dst.add(6) as *mut u16) |= bits;
    }
}
