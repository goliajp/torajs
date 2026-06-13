//! DynObj heap-block layout constants.
//!
//! Compact insertion-ordered dict — the CPython 3.7 dict / V8
//! property-bag shape: a **dense entry array** appended in insertion
//! order plus a power-of-2 **hash index** mapping probe slots to entry
//! indices. JS property insertion order is observable semantics
//! (property printing / `Object.keys` / `for-in` share this root), so
//! a bare linear-probe table cannot back a dynobj.
//!
//! ```text
//! offset  | size    | field
//! --------|---------|------
//!   0     |  8B     | universal heap header (refcount + type_tag + flags)
//!   8     |  4B     | count       (u32) — # of live entries (holes excluded)
//!  12     |  4B     | cap         (u32) — hash-index slot count (power of 2)
//!  16     |  4B     | entries_len (u32) — dense-array used length (holes included)
//!  20     |  4B     | entries_cap (u32) — dense-array capacity = cap * 7/8
//!  24     |  4×cap  | index[cap]  (u32) — IDX_EMPTY / IDX_TOMBSTONE / entry idx
//!  24+4×cap | 16×ecap | entries[entries_cap] — { key_ptr_tagged, value_anyv }
//! ```
//!
//! Deletion: the index slot becomes [`IDX_TOMBSTONE`] (probe walks
//! past) and the dense entry becomes a **hole** (`key_ptr_tagged = 0`,
//! skipped by iteration / drop / compact). Holes are reclaimed when
//! the dense array fills and [`crate::resize`] compacts. Re-inserting
//! a deleted key appends a fresh entry at the tail — exactly the JS
//! "delete then set moves the key to the end" order semantics.
//!
//! ## Entry key-ptr low-bit flag tagging (Step 7e-B)
//!
//! `key_ptr_tagged: u64` packs the Str pointer (8-aligned ⇒ low 3 bits
//! free) with the spec §6.2.5 PropertyDescriptor `writable` /
//! `enumerable` / `configurable` flags in bits 0/1/2. The high 61 bits
//! hold the real Str pointer (mask `!0x7`). This is the JSC / V8
//! hidden-class style for compact descriptor storage; same pedigree as
//! Swift Class isa + V8 Maps.
//!
//! ## Entry value field
//!
//! `value_anyv: u64` is a NaN-box [`torajs_anyvalue::AnyValue`] encoding
//! the slot's `(tag, value)` pair as a single 64-bit immediate. Cells
//! (heap pointers) are stored verbatim; immediates (int32 / double /
//! bool / null / undef) are NaN-encoded per the AnyValue ABI. Decode
//! via `__torajs_anyv_unbox_tag` / `_unbox_value` externs.

/// Universal heap header size (`{ refcount: u32, type_tag: u16, flags: u16 }`).
pub const HEAP_HEADER_SIZE: usize = 8;

/// Header bytes before `index[]` — heap header (8) +
/// count/cap/entries_len/entries_cap (4 × u32).
pub const DYNOBJ_HDR_SIZE: usize = 24;

/// Per-entry size in the dense array. `key_ptr_tagged: u64` (8) +
/// `value_anyv: u64` (8). 4 entries per 64-byte cache line.
pub const DYNOBJ_ENTRY_SIZE: usize = 16;

/// Initial hash-index slot count on alloc. Must be a power of 2 — the
/// linear-probe `slot = (h + step) & (cap - 1)` mask depends on it.
pub const DYNOBJ_INITIAL_CAP: u32 = 8;

/// `type_tag` value for DynObj heap blocks (matches
/// `torajs_rc::Tag::DynObj` = 14).
pub const TAG_DYNOBJ: u16 = 14;

/// Heap-header `flags` bit (u16 @6) marking a DynObj created with a
/// null prototype (`Object.create(null)` semantics — e.g. a regex
/// match's `.groups` dict per spec §22.2.7.8). Print surfaces render
/// the `[Object: null prototype] ` prefix off this bit. Bit 6 is free
/// on DynObj: bits 1-5 are SPLIT_BLOCK / STATIC_LITERAL / color-field
/// (3-4 overlay) / FROZEN / BUFFERED per torajs-rc's flag table.
pub const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

/// Heap-header `flags` bit (u16 @6) mirror of
/// `torajs_rc::FLAG_NON_EXTENSIBLE` (bit 8). Mirrored here so
/// `__torajs_dynobj_define` can gate fresh-key inserts on a sealed /
/// `preventExtensions`'d dict without taking a Cargo dep on torajs-rc.
/// Update both sides if the bit position ever moves.
pub const DYNOBJ_HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;

/// Offset of the `count` u32 within the heap block.
pub const DYNOBJ_COUNT_OFF: usize = HEAP_HEADER_SIZE;

/// Offset of the `cap` u32 within the heap block.
pub const DYNOBJ_CAP_OFF: usize = HEAP_HEADER_SIZE + 4;

/// Offset of the `entries_len` u32 within the heap block.
pub const DYNOBJ_ENTRIES_LEN_OFF: usize = HEAP_HEADER_SIZE + 8;

/// Offset of the `entries_cap` u32 within the heap block.
pub const DYNOBJ_ENTRIES_CAP_OFF: usize = HEAP_HEADER_SIZE + 12;

/// Offset of the `index[cap]` u32 array within the heap block.
pub const DYNOBJ_INDEX_OFF: usize = DYNOBJ_HDR_SIZE;

/// Hash-index "slot never used" sentinel (CPython `DKIX_EMPTY`
/// analogue). All-ones so a `write_bytes(.., 0xFF, ..)` fill
/// initializes a fresh index in one call.
pub const IDX_EMPTY: u32 = 0xFFFF_FFFF;

/// Hash-index "entry deleted here" sentinel (CPython `DKIX_DUMMY`
/// analogue). Probe walks past it; insert may reuse the slot.
pub const IDX_TOMBSTONE: u32 = 0xFFFF_FFFE;

/// Dense-entry hole marker for `Entry::key_ptr_tagged` — left behind
/// by delete, skipped by iteration / drop, reclaimed by compact. `0`
/// is disjoint from live keys (real Str heap addrs are 8-aligned and
/// well above zero, so `ptr | flags != 0`).
pub const DYNOBJ_KEY_HOLE: u64 = 0;

/// Mask isolating the real Str pointer in a `key_ptr_tagged` word
/// (clears the low-3 flag bits).
pub const BUCKET_KEY_PTR_MASK: u64 = !0x7u64;

/// Mask isolating the three PropertyDescriptor flag bits in a
/// `key_ptr_tagged` word.
pub const BUCKET_FLAGS_MASK: u64 = 0x7;

/// `ANY_UNDEF` tag (matches `torajs_rc::AnySlotTag::Undef = 5`). Returned
/// by `get_tag` when the key is absent or `obj` is not a dynobj.
pub const ANY_UNDEF: u64 = 5;

/// Synthetic `get_tag` sentinel marking an **accessor** entry — the
/// stored `value_anyv` is a cell pointing at an `AccessorPair` (RFC
/// C3). Lives one past `ANY_UNDEF` in the otherwise 0..5 AnySlotTag
/// space; never produced by the NaN-box encoder, so data-property
/// paths never see it. The SSA property-GET emit branches on it to
/// dispatch the getter.
pub const ANY_ACCESSOR: u64 = 6;

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
/// [`crate::set::__torajs_dynobj_set`] to detect when the prior entry
/// value is a heap pointer that owes an rc-dec before overwrite.
pub const ANY_HEAP: u64 = 4;

// Object.defineProperty descriptor-flags encoding — `flags_byte`
// passed by ssa_lower to [`crate::define::__torajs_dynobj_define`].
// Low 3 bits = flag VALUE; bits 3-5 = flag PRESENT in descriptor;
// bit 6 = value present in descriptor.

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

/// Dense-array capacity for a given hash-index `cap`: `cap * 7/8`.
/// Index occupancy (live slots + tombstones) never exceeds
/// `entries_len ≤ entries_cap`, so at least `cap / 8` index slots stay
/// empty and every probe walk terminates.
#[inline]
pub const fn entries_cap_for(cap: u32) -> u32 {
    cap - cap / 8
}

/// Total heap-block byte size for a given hash-index `cap`:
/// header + `index[cap]` + `entries[entries_cap]`.
#[inline]
pub const fn block_bytes(cap: u32) -> usize {
    DYNOBJ_HDR_SIZE + cap as usize * 4 + entries_cap_for(cap) as usize * DYNOBJ_ENTRY_SIZE
}
