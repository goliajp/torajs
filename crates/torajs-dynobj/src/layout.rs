//! DynObj heap-block layout constants.
//!
//! Compact insertion-ordered dict — the CPython 3.7 dict / V8
//! property-bag shape: a **dense entry array** appended in insertion
//! order plus a power-of-2 **hash index** mapping probe slots to entry
//! indices. JS property insertion order is observable semantics
//! (property printing / `Object.keys` / `for-in` share this root), so
//! a bare linear-probe table cannot back a dynobj.
//!
//! Two blocks (RFC 20260809-dynobj-store-split). The **header cell**
//! is a fixed 32-byte allocation whose address never changes for the
//! object's lifetime — identity, refcount, every owner's pointer, the
//! cycle collector's buffer and weak references all stay stable across
//! growth. The **store** is an independent allocation holding the hash
//! index + dense entries; [`crate::resize`] swaps it out wholesale
//! (CPython's `ma_keys` / V8's properties backing store shape). The
//! previous single-block layout made resize relocate the header, which
//! split identity across owners and left every non-updated owner on a
//! freed block (multi-owner UAF).
//!
//! ```text
//! header cell (32B, address-stable)
//! offset  | size | field
//! --------|------|------
//!   0     |  8B  | universal heap header (refcount + type_tag + flags)
//!   8     |  4B  | count       (u32) — # of live entries (holes excluded)
//!  12     |  4B  | cap         (u32) — hash-index slot count (power of 2)
//!  16     |  4B  | entries_len (u32) — dense-array used length (holes included)
//!  20     |  4B  | entries_cap (u32) — dense-array capacity = cap * 7/8
//!  24     |  8B  | store       (*mut u8) — the index+entries block
//!
//! store block (store_bytes(cap), swapped on resize)
//! offset  | size    | field
//! --------|---------|------
//!   0     |  4×cap  | index[cap] (u32) — IDX_EMPTY / IDX_TOMBSTONE / entry idx
//!  4×cap  | 16×ecap | entries[entries_cap] — { key_ptr_tagged, value_anyv }
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
//! `key_ptr_tagged: u64` packs the key-cell pointer (8-aligned ⇒ low 3
//! bits free) with the spec §6.2.5 PropertyDescriptor `writable` /
//! `enumerable` / `configurable` flags in bits 0/1/2. The high 61 bits
//! hold the real key pointer (mask `!0x7`). This is the JSC / V8
//! hidden-class style for compact descriptor storage; same pedigree as
//! Swift Class isa + V8 Maps.
//!
//! ## Key domain — Str *or* Symbol cell
//!
//! §6.1.7 makes a property key either a String or a Symbol, and both
//! are 8-aligned heap cells here, so one slot backs both: the
//! **pointed-to cell's own `type_tag`** discriminates
//! ([`crate::probe::key_is_symbol`]). String keys hash by content and
//! compare by content; symbol keys hash by pointer and compare by
//! pointer identity — a Symbol's identity *is* its cell (§20.4, each
//! `Symbol(desc)` allocates fresh, description is not identity).
//! Cross-kind keys never compare equal.
//!
//! §10.1.11.1 OrdinaryOwnPropertyKeys orders own keys in three
//! buckets — array indices ascending, then strings in insertion
//! order, then symbols in insertion order.
//! [`crate::iter::__torajs_dynobj_iter_order`] materializes the first
//! two (every user-visible string-key surface reads it) and
//! [`crate::iter::__torajs_dynobj_iter_symbol_order`] the third.
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

/// Header-cell allocation size — heap header (8) +
/// count/cap/entries_len/entries_cap (4 × u32) + store pointer (8).
/// Fixed for the object's lifetime; the address never changes.
pub const DYNOBJ_HEADER_BYTES: usize = 32;

/// Offset of the `store: *mut u8` pointer within the header cell.
pub const DYNOBJ_STORE_OFF: usize = 24;

/// Per-entry size in the dense array. `key_ptr_tagged: u64` (8) +
/// `value_anyv: u64` (8). 4 entries per 64-byte cache line.
pub const DYNOBJ_ENTRY_SIZE: usize = 16;

/// Initial hash-index slot count on alloc. Must be a power of 2 — the
/// linear-probe `slot = (h + step) & (cap - 1)` mask depends on it.
pub const DYNOBJ_INITIAL_CAP: u32 = 8;

/// `type_tag` value for DynObj heap blocks (matches
/// `torajs_rc::Tag::DynObj` = 14).
pub const TAG_DYNOBJ: u16 = 14;

/// `type_tag` mirror for Arr heap cells (`torajs_rc::Tag::Arr` = 2) —
/// the define/descriptor paths dispatch per receiver shape.
pub const TAG_ARR_HDR: u16 = 2;

/// `type_tag` mirror for Closure heap cells (`torajs_rc::Tag::Closure`
/// = 3). A Closure's expando props dynobj lives at +24
/// (`CLOSURE_PROPS_OFF` mirror — same slot Arr uses).
pub const TAG_CLOSURE_HDR: u16 = 3;

/// `type_tag` mirror for Symbol key cells (`torajs_rc::Tag::Symbol`
/// = 7, `torajs_str::symbol::TAG_SYMBOL`). A property key is either a
/// Str cell or a Symbol cell (§6.1.7 property-key domain); the key
/// slot stores the pointer either way and the pointed-to cell's own
/// `type_tag` is the discriminator — see [`crate::probe::key_is_symbol`].
pub const TAG_SYMBOL_KEY: u16 = 7;

/// `type_tag` mirror for Obj (static-layout class instance / typed
/// property bag = `torajs_rc::Tag::Obj` = 1). Used by
/// `define_all`'s desc gate so an ObjectLit-shaped desc value
/// (`{value: {}, enumerable: true}`) clears the same accept path as
/// DynObj / Arr / Closure receivers; the dispatcher's `_ => null`
/// fallback treats it as an empty descriptor (spec §6.2.6.5).
pub const TAG_OBJ: u16 = 1;

/// Arr / Closure expando props-dynobj slot offset (torajs-arr
/// `ARR_PROPS_OFF` / torajs-core `CLOSURE_PROPS_OFF` mirror).
pub const CELL_PROPS_OFF: usize = 24;

/// `type_tag` mirror for Promise heap cells (torajs-promise
/// `layout.rs::TAG_PROMISE` = 8). Its lazy expando props dynobj
/// lives at +32 ([`PROMISE_PROPS_OFF`]) — +24 is the callback list.
pub const TAG_PROMISE_HDR: u16 = 8;

/// Promise expando props-dynobj slot offset (torajs-promise
/// `layout.rs::Promise::props` mirror, lockstep).
pub const PROMISE_PROPS_OFF: usize = 32;

/// HOLE sentinel bit pattern for a shadow entry's dead value slot
/// (RFC 20260713 chunk C — `delete arr[i]`). Deliberately NOT a
/// NaN-box encoder product: `TAG_BIT_TYPE_OTHER | TAG_BIT_UNDEFINED
/// | 0x10` — the 0x10 bit is unused by every `VALUE_*` encoding, so
/// no boxed value can collide (the encoder canonicalizes undefined
/// to 0x0A, which is why `box_from_pair(5, 1)` cannot serve as a
/// sentinel). An immediate bit pattern — the drop walk's cell gate
/// filters it like any other non-heap value.
pub const DYNOBJ_HOLE_SENTINEL: u64 = 0x1A;

/// Heap-header `flags` bit (u16 @6) marking a DynObj created with a
/// null prototype (`Object.create(null)` semantics — e.g. a regex
/// match's `.groups` dict per spec §22.2.7.8). Print surfaces render
/// the `[Object: null prototype] ` prefix off this bit. Bit 6 is free
/// on DynObj: bits 1-5 are SPLIT_BLOCK / STATIC_LITERAL / ARR_ANY /
/// FROZEN / BUFFERED per torajs-rc's flag table (color field lives
/// at bits 13-14 since RFC 20260706 chunk 573).
pub const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

/// Heap-header `flags` bit (u16 @6) mirror of
/// `torajs_rc::FLAG_NON_EXTENSIBLE` (bit 8). Mirrored here so
/// `__torajs_dynobj_define` can gate fresh-key inserts on a sealed /
/// `preventExtensions`'d dict without taking a Cargo dep on torajs-rc.
/// Update both sides if the bit position ever moves.
pub const DYNOBJ_HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;

/// Heap-header `flags` bit (u16 @6) mirror of
/// `torajs_rc::FLAG_DYNOBJ_CLASS_CTOR` (bit 10, Tag::DynObj-private —
/// disjoint-by-tag reuse of Closure's name-deleted / Arr's
/// element-kind bits). Marks the `__class_<C>` class-constructor
/// singleton dynobj so `typeof` answers `"function"` (RFC
/// 20260717-class-first-class-value knife A). Update both sides if
/// the bit position ever moves.
pub const DYNOBJ_HDR_FLAG_CLASS_CTOR: u16 = 1 << 10;

/// Offset of the `count` u32 within the heap block.
pub const DYNOBJ_COUNT_OFF: usize = HEAP_HEADER_SIZE;

/// Offset of the `cap` u32 within the heap block.
pub const DYNOBJ_CAP_OFF: usize = HEAP_HEADER_SIZE + 4;

/// Offset of the `entries_len` u32 within the heap block.
pub const DYNOBJ_ENTRIES_LEN_OFF: usize = HEAP_HEADER_SIZE + 8;

/// Offset of the `entries_cap` u32 within the heap block.
pub const DYNOBJ_ENTRIES_CAP_OFF: usize = HEAP_HEADER_SIZE + 12;

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

/// Mask isolating the real key-cell pointer (Str or Symbol) in a
/// `key_ptr_tagged` word (clears the low-3 flag bits).
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
/// "Descriptor includes a `get` field" sentinel (bit 7) — accessor
/// descriptors only (RFC 20260713 chunk D). §10.1.6.3 partial
/// redefine keeps the current getter when `get` is absent; an
/// explicit `get: undefined` is present + NULL (clears it).
pub const DEFINE_PRESENT_GET: u64 = 1 << 7;
/// "Descriptor includes a `set` field" sentinel (bit 8).
pub const DEFINE_PRESENT_SET: u64 = 1 << 8;

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

/// Store-block byte size for a given hash-index `cap`:
/// `index[cap]` + `entries[entries_cap]`.
#[inline]
pub const fn store_bytes(cap: u32) -> usize {
    cap as usize * 4 + entries_cap_for(cap) as usize * DYNOBJ_ENTRY_SIZE
}
