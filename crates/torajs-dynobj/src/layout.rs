//! DynObj heap-block layout constants.
//!
//! Mirrors `runtime_str.c`'s `__TORAJS_DYNOBJ_*` macros 1:1 (the C
//! runtime keeps the definitions inline so this is a deliberate
//! duplicate; the contract is "shared layout, separately compiled" —
//! same pattern as `torajs-arr::layout`).
//!
//! ```text
//! offset | size | field
//! -------|------|------
//!   0    |  8B  | universal heap header (refcount + type_tag + flags)
//!   8    |  4B  | count (u32) — # of live entries
//!  12    |  4B  | cap   (u32) — bucket array size (power of 2)
//!  16    |  4B  | tomb  (u32) — # of tombstone slots
//!  20    |  4B  | pad
//!  24    | 24×n | buckets[cap] — `{ key_ptr, tag, value }` (24B each)
//! ```

/// Universal heap header size (`{ refcount: u32, type_tag: u16, flags: u16 }`).
pub const HEAP_HEADER_SIZE: usize = 8;

/// Header bytes before `buckets[]` (matches C macro
/// `__TORAJS_DYNOBJ_HDR_SIZE`). Header + count/cap/tomb/pad = 24.
pub const DYNOBJ_HDR_SIZE: usize = 24;

/// Per-bucket size (matches C macro `__TORAJS_DYNOBJ_BUCKET_SIZE`):
/// `key_ptr: *Str` (8) + `tag: u64` (8) + `value: u64` (8).
pub const DYNOBJ_BUCKET_SIZE: usize = 24;

/// Initial bucket count on alloc (matches C macro
/// `__TORAJS_DYNOBJ_INITIAL_CAP`). Must be a power of 2 — the linear-
/// probe `idx = (h + step) & (cap - 1)` mask depends on it.
pub const DYNOBJ_INITIAL_CAP: u32 = 8;

/// `type_tag` value for DynObj heap blocks (matches
/// `torajs_rc::Tag::DynObj` = 14 and `runtime_str.c::__TORAJS_TAG_DYNOBJ`).
pub const TAG_DYNOBJ: u16 = 14;

/// Offset of the `count` u32 within the heap block.
pub const DYNOBJ_COUNT_OFF: usize = HEAP_HEADER_SIZE;

/// Offset of the `cap` u32 within the heap block.
pub const DYNOBJ_CAP_OFF: usize = HEAP_HEADER_SIZE + 4;

/// Offset of the `tomb` u32 within the heap block.
pub const DYNOBJ_TOMB_OFF: usize = HEAP_HEADER_SIZE + 8;
