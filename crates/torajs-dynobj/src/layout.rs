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

/// Tombstone sentinel for `Bucket::key_ptr`. NULL = empty, `1` =
/// tombstone (slot was occupied + deleted; probe must walk past it),
/// otherwise = owning `*Str` pointer.
pub const DYNOBJ_TOMBSTONE: *mut core::ffi::c_void = 1usize as *mut core::ffi::c_void;

/// `ANY_UNDEF` tag (matches `torajs_rc::AnySlotTag::Undef = 5`). Returned
/// by `get_tag` when the key is absent or `obj` is not a dynobj.
pub const ANY_UNDEF: u64 = 5;

// Bucket-tag layout: low 8 bits = ANY_TAG (0-5); bits 8-10 = spec
// §6.2.5 PropertyDescriptor data-attribute flags writable / enumerable
// / configurable. Avoids growing the 24-byte bucket struct.

/// Mask for the low-8 ANY_TAG bits in `Bucket::tag`. Callers reading
/// the slot tag must mask before tag-dispatch.
pub const BUCKET_TAG_MASK: u64 = 0xff;

/// Bit position of the `writable` PropertyDescriptor flag inside
/// `Bucket::tag`.
pub const BUCKET_FLAG_WRITABLE: u64 = 1 << 8;
/// Bit position of the `enumerable` PropertyDescriptor flag inside
/// `Bucket::tag`.
pub const BUCKET_FLAG_ENUMERABLE: u64 = 1 << 9;
/// Bit position of the `configurable` PropertyDescriptor flag inside
/// `Bucket::tag`.
pub const BUCKET_FLAG_CONFIGURABLE: u64 = 1 << 10;

/// All three data-attribute flags set — matches C macro
/// `__TORAJS_BUCKET_FLAGS_DEFAULT`. Used by implicit-set (`obj.x = v`)
/// + object-literal init per spec §10.1.5.1 / §10.1.6.2 CreateData-
/// Property (writable / enumerable / configurable default true).
pub const BUCKET_FLAGS_DEFAULT: u64 =
    BUCKET_FLAG_WRITABLE | BUCKET_FLAG_ENUMERABLE | BUCKET_FLAG_CONFIGURABLE;

/// `ANY_HEAP` tag (matches `torajs_rc::AnySlotTag::Heap = 4`). Used by
/// [`crate::set::__torajs_dynobj_set`] to detect when the prior bucket
/// value is a heap pointer that owes an rc-dec before overwrite.
pub const ANY_HEAP: u64 = 4;

// Object.defineProperty descriptor-flags encoding — `flags_byte`
// passed by ssa_lower to [`crate::define::__torajs_dynobj_define`].
// Low 3 bits = flag VALUE; bits 3-5 = flag PRESENT in descriptor;
// bit 6 = value present in descriptor. Matches the C macros
// `__TORAJS_DEFINE_*` 1:1.

/// Descriptor's `writable` flag value (low bit 0 of `flags_byte`).
pub const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
/// Descriptor's `enumerable` flag value (low bit 1).
pub const DEFINE_FLAG_ENUMERABLE: u64 = 1 << 1;
/// Descriptor's `configurable` flag value (low bit 2).
pub const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
/// "Writable flag present in descriptor" sentinel (bit 3). Spec
/// §10.1.6.3 distinguishes "absent" (leave current alone on redefine,
/// default false on fresh) from "present-false" (use the value).
pub const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
/// "Enumerable flag present in descriptor" sentinel (bit 4).
pub const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
/// "Configurable flag present in descriptor" sentinel (bit 5).
pub const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
/// "Descriptor includes [[Value]] field" sentinel (bit 6).
pub const DEFINE_PRESENT_VALUE: u64 = 1 << 6;

// `Str` layout — mirrored from `torajs-str::layout` (separately
// compiled, shared contract; same dep-avoidance pattern torajs-arr uses
// for `HeapHeader`). Updates to torajs-str's Str layout require a
// mirroring edit here.

/// Offset of the `len: u64` field inside a Str heap block.
pub const STR_LEN_OFF: usize = 8;
/// Offset of the inline UTF-8 byte payload inside a Str heap block.
pub const STR_DATA_OFF: usize = 16;
