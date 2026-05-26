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
//!  24    | 16×n | buckets[cap] — `{ key_ptr_tagged, value_anyv }` (16B each)
//! ```
//!
//! ## Bucket key-ptr low-bit flag tagging (Step 7e-B)
//!
//! `key_ptr_tagged: u64` packs the Str pointer (8-aligned ⇒ low 3 bits
//! free) with the spec §6.2.5 PropertyDescriptor `writable` /
//! `enumerable` / `configurable` flags in bits 0/1/2. The high 61 bits
//! hold the real Str pointer (mask `!0x7`). This is the JSC / V8
//! hidden-class style for compact descriptor storage; same pedigree as
//! Swift Class isa + V8 Maps.
//!
//! Sentinels (key_ptr_tagged values):
//! * `0` — empty (calloc default; flag bits naturally zero).
//! * `1` — tombstone (probe walks past). Disjoint from empty (`!= 0`)
//!   and from live (`ptr & !0x7 != 0` since real Str heap addrs ≥ 16).
//! * `else` — live: real ptr in `& !0x7`, flags in `& 0x7`.
//!
//! ## Bucket value field
//!
//! `value_anyv: u64` is a NaN-box [`torajs_anyvalue::AnyValue`] encoding
//! the slot's `(tag, value)` pair as a single 64-bit immediate. Cells
//! (heap pointers) are stored verbatim; immediates (int32 / double /
//! bool / null / undef) are NaN-encoded per the AnyValue ABI. Decode
//! via `__torajs_anyv_unbox_tag` / `_unbox_value` externs.

/// Universal heap header size (`{ refcount: u32, type_tag: u16, flags: u16 }`).
pub const HEAP_HEADER_SIZE: usize = 8;

/// Header bytes before `buckets[]` (matches C macro
/// `__TORAJS_DYNOBJ_HDR_SIZE`). Header + count/cap/tomb/pad = 24.
pub const DYNOBJ_HDR_SIZE: usize = 24;

/// Per-bucket size (Step 7e-B: 24 → 16). `key_ptr_tagged: u64` (8) +
/// `value_anyv: u64` (8). Halves the cache footprint per bucket, lets
/// 4 buckets fit per 64-byte cache line (vs 2 pre-7e-B).
pub const DYNOBJ_BUCKET_SIZE: usize = 16;

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

/// Empty sentinel for `Bucket::key_ptr_tagged`. `0` matches calloc's
/// default so a fresh dynobj block has every bucket marked empty.
pub const DYNOBJ_KEY_EMPTY: u64 = 0;

/// Tombstone sentinel for `Bucket::key_ptr_tagged`. `1` is disjoint
/// from both empty (`== 0`) and live (`ptr & !0x7 != 0` since real Str
/// heap addrs are 8-aligned and well above zero).
pub const DYNOBJ_KEY_TOMBSTONE: u64 = 1;

/// Mask isolating the real Str pointer in a `key_ptr_tagged` word
/// (clears the low-3 flag bits).
pub const BUCKET_KEY_PTR_MASK: u64 = !0x7u64;

/// Mask isolating the three PropertyDescriptor flag bits in a
/// `key_ptr_tagged` word.
pub const BUCKET_FLAGS_MASK: u64 = 0x7;

/// `ANY_UNDEF` tag (matches `torajs_rc::AnySlotTag::Undef = 5`). Returned
/// by `get_tag` when the key is absent or `obj` is not a dynobj.
pub const ANY_UNDEF: u64 = 5;

// Bucket flag layout (Step 7e-B): the three PropertyDescriptor data-
// attribute flags occupy bits 0/1/2 of `Bucket::key_ptr_tagged`. The
// per-slot ANY_TAG (0-5) now lives inside the NaN-box `value_anyv`
// — decode via `__torajs_anyv_unbox_tag`. `BUCKET_TAG_MASK` is kept
// only as the input-side mask callers apply to the `tag: u64` FFI
// parameter before packing into `value_anyv`.

/// Mask for the low-8 ANY_TAG bits of the FFI `tag: u64` parameter
/// passed into [`crate::set::__torajs_dynobj_set`] /
/// [`crate::define::__torajs_dynobj_define`]. Callers may pass dirty
/// high bits; mask before forwarding into the NaN-box pair encoder.
pub const BUCKET_TAG_MASK: u64 = 0xff;

/// `writable` PropertyDescriptor flag — bit 0 of `key_ptr_tagged`.
pub const BUCKET_FLAG_WRITABLE: u64 = 1 << 0;
/// `enumerable` PropertyDescriptor flag — bit 1 of `key_ptr_tagged`.
pub const BUCKET_FLAG_ENUMERABLE: u64 = 1 << 1;
/// `configurable` PropertyDescriptor flag — bit 2 of `key_ptr_tagged`.
pub const BUCKET_FLAG_CONFIGURABLE: u64 = 1 << 2;

/// All three data-attribute flags set. Used by implicit-set
/// (`obj.x = v`) + object-literal init per spec §10.1.5.1 / §10.1.6.2
/// CreateDataProperty (writable / enumerable / configurable default true).
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
