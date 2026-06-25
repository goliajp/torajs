//! Array heap-block layout constants.
//!
//! Mirrors `runtime_str.c`'s arr macros 1:1 (the C runtime keeps the
//! definitions inline so this is a deliberate duplicate; the contract
//! is "shared layout, separately compiled"):
//!
//! ```text
//! offset | size | field
//! -------|------|------
//!   0    |  8B  | universal heap header (refcount + type_tag + flags)
//!   8    |  8B  | len (current element count, u64)
//!  16    |  8B  | cap (u32) + head_offset (u32) — packed T-13.5
//!  24    |  8B  | props_dynobj — pointer to inline arr-prop dynobj or
//!                  NULL when no `arr.x = v` was ever written
//!                  (Round 4 chunk 5a layout, 2026-06-25;
//!                  chunk 5b+ flips arrprops_set to write here inline)
//!  32    | 8×n  | slots[cap] — 8 bytes each (boxed-Any tag/value or
//!                  inline scalar depending on the element type)
//! ```
//!
//! Sub-types (encoded via the universal header's `type_tag` /
//! `flags`):
//! - `Array<T>` (T is a tightly-typed scalar) → slots are raw values
//! - `Array<Any>` (FLAG_ARR_ANY set) → each slot is a tag/value pair
//!   spread over 16 bytes; len/cap are still element counts but each
//!   element is 16 B instead of 8. See `runtime_str.c` arr_alloc_any.
//! - `Array<Substr>` split-block (FLAG_SPLIT_BLOCK in `torajs-str`) —
//!   same 32-byte header (chunk 5a sync); slots are `*mut u8` pointers
//!   into inline substrs trailing the slot table. See
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

/// Offset of `slots[0]` — the inline-following element array.
/// Bumped from 24 → 32 in Round 4 chunk 5a to make room for the
/// inline `props_dynobj` slot at offset 24. Cross-crate sync sites
/// (must move in lockstep):
/// - `torajs_core::ssa_lower::ARR_DATA_OFF` (IR emit)
/// - `torajs_str::split::pool::ARR_HDR_SIZE` (split-block layout)
pub const ARR_SLOTS_OFF: usize = HEAP_HEADER_SIZE + 24;
