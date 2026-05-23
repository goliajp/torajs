//! Map / Set heap-block layout constants + struct shapes.
//!
//! Mirrors `runtime_map.c`'s layout 1:1 (same separately-compiled
//! contract pattern torajs-arr / torajs-dynobj use for shared headers).
//! `#[repr(C)]` on the structs guarantees ABI-compat with the existing
//! C-side definitions (still present in runtime_map.c during the
//! progressive P4.3 port; collapse at P4.3-i closer).
//!
//! ```text
//! Map struct (64 bytes, 8-byte aligned):
//!   offset 0  : universal heap header (8B; refcount + type_tag + flags)
//!   offset 8  : n_entries    (u32) — live entry count (`size()` returns this)
//!   offset 12 : n_used       (u32) — entries[] occupied prefix incl. tombstones
//!   offset 16 : entries_cap  (u32)
//!   offset 20 : slots_count  (u32)
//!   offset 24 : n_tombstones (u32) — slot-side tombstone count
//!   offset 28 : _pad         (u32)
//!   offset 32 : slots        (*MapSlot)
//!   offset 40 : entries      (*MapEntry)
//!
//! MapSlot — 8 bytes:
//!   (hash:u32 hi32) | (entry_idx:u32 lo32)
//!     hash: 0 reserved as ENTRY_HASH_TOMBSTONE on the entry side;
//!           slot side uses SLOT_EMPTY / SLOT_TOMBSTONE sentinels on
//!           the low 32 bits.
//!     entry_idx: SLOT_EMPTY (0xFFFFFFFF) / SLOT_TOMBSTONE (0xFFFFFFFE)
//!                or a valid index into entries[].
//!
//! MapEntry (40 bytes, packed):
//!   offset 0  : hash         (u32) — 0 = entry-side tombstone, else live
//!   offset 4  : _pad         (u32)
//!   offset 8  : key_tag      (u8)  — ANY_NULL/UNDEF/BOOL/I64/F64/HEAP
//!   offset 9  : _kpad        (7B)
//!   offset 16 : key_payload  (u64) — per-tag payload
//!   offset 24 : value_tag    (u8)
//!   offset 25 : _vpad        (7B)
//!   offset 32 : value_payload(u64)
//! ```

use core::ffi::c_void;

/// `type_tag` value for Map/Set heap blocks (matches
/// `torajs_rc::Tag::Map` = 15 and `runtime_map.c::__TORAJS_TAG_MAP`).
/// Set wears the same tag — Set vs Map distinction is SSA-side only.
pub const TAG_MAP: u16 = 15;

/// ANY-slot tag values (mirror of `torajs_rc::AnySlotTag`).
pub const ANY_NULL: u8 = 0;
pub const ANY_BOOL: u8 = 1;
pub const ANY_I64: u8 = 2;
pub const ANY_F64: u8 = 3;
pub const ANY_HEAP: u8 = 4;
pub const ANY_UNDEF: u8 = 5;

/// `STR` heap type_tag — used by [`crate::hash`] and [`crate::eq`] to
/// detect string keys for byte-content hashing / comparison vs the
/// pointer-identity fallback for other heap types.
pub const TAG_STR: u16 = 0;

/// Initial `entries[]` capacity at create.
pub const MAP_ENTRIES_INITIAL: u32 = 8;

/// Initial `slots[]` count at create. MUST be a power of 2.
pub const MAP_SLOTS_INITIAL: u32 = 16;

/// Load-factor numerator: rehash when `n_used * 4 > slots_count * 3`.
pub const MAP_LOAD_NUMER: u32 = 3;
pub const MAP_LOAD_DENOM: u32 = 4;

/// Entry-side tombstone marker. A `MapEntry::hash == 0` means the
/// entry was deleted; iter walks must skip. Live entries always have
/// `hash >= 1` (the hash routine `| 1`s the low bit before return).
pub const ENTRY_HASH_TOMBSTONE: u32 = 0;

/// Slot-side empty sentinel — stored in low 32 bits of `MapSlot`.
pub const SLOT_EMPTY: u32 = 0xFFFF_FFFF;

/// Slot-side tombstone sentinel — stored in low 32 bits of `MapSlot`.
/// Iter / lookup walks past this and the first one found is the
/// preferred reinsert candidate (lazy compaction).
pub const SLOT_TOMBSTONE: u32 = 0xFFFF_FFFE;

/// 64-bit slot: `(hash << 32) | entry_idx`.
pub type MapSlot = u64;

/// Pack (hash, entry_idx) into a slot.
#[inline]
pub const fn slot_make(hash: u32, idx: u32) -> MapSlot {
    ((hash as u64) << 32) | (idx as u64)
}

/// Unpack hash from a slot.
#[inline]
pub const fn slot_hash(s: MapSlot) -> u32 {
    (s >> 32) as u32
}

/// Unpack entry index from a slot.
#[inline]
pub const fn slot_index(s: MapSlot) -> u32 {
    (s & 0xFFFF_FFFF) as u32
}

/// In-block heap header — 8 bytes, ABI-shared with `torajs_rc::HeapHeader`.
#[repr(C, align(8))]
pub struct HeapHeader {
    pub refcount: u32,
    pub type_tag: u16,
    pub flags: u16,
}

/// `MapEntry` — 40 bytes packed with explicit alignment-padding bytes
/// matching the C-side `MapEntry` struct exactly. The `_pad / _kpad /
/// _vpad` fields are ABI alignment-fillers (referenced only by the
/// struct layout, never by name); `hash` is written by mutate.rs +
/// rehash and read by probe (some currently `#[allow(dead_code)]`
/// pending P4.3-c..-f consumer ports). Whole-struct allow keeps the
/// layout in one declaration without per-field clutter.
#[allow(dead_code)]
#[repr(C)]
pub struct MapEntry {
    pub hash: u32,
    pub _pad: u32,
    pub key_tag: u8,
    pub _kpad: [u8; 7],
    pub key_payload: u64,
    pub value_tag: u8,
    pub _vpad: [u8; 7],
    pub value_payload: u64,
}

/// `Map` struct — fits the C-side `Map` shape 1:1.
#[repr(C)]
pub struct Map {
    pub header: HeapHeader,
    pub n_entries: u32,
    pub n_used: u32,
    pub entries_cap: u32,
    pub slots_count: u32,
    pub n_tombstones: u32,
    pub _pad: u32,
    pub slots: *mut MapSlot,
    pub entries: *mut MapEntry,
}

/// `STR_LEN_OFF` mirror (Str layout — for hash_bytes / eq in later
/// sub-steps). Hardcoded duplicate of torajs-str — same dep-avoidance
/// pattern torajs-dynobj uses.
pub const STR_LEN_OFF: usize = 8;
pub const STR_DATA_OFF: usize = 16;

/// Cast a `*mut c_void` heap pointer to a `*mut Map`.
#[inline]
pub unsafe fn as_map(p: *mut c_void) -> *mut Map {
    p as *mut Map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layouts_match_c() {
        assert_eq!(core::mem::size_of::<HeapHeader>(), 8);
        assert_eq!(core::mem::size_of::<MapEntry>(), 40);
        // Map: hdr(8) + 6×u32(24) + 2×ptr(16) = 48; align 8.
        assert_eq!(core::mem::size_of::<Map>(), 48);
        assert_eq!(core::mem::align_of::<Map>(), 8);

        assert_eq!(core::mem::offset_of!(MapEntry, hash), 0);
        assert_eq!(core::mem::offset_of!(MapEntry, key_tag), 8);
        assert_eq!(core::mem::offset_of!(MapEntry, key_payload), 16);
        assert_eq!(core::mem::offset_of!(MapEntry, value_tag), 24);
        assert_eq!(core::mem::offset_of!(MapEntry, value_payload), 32);

        assert_eq!(core::mem::offset_of!(Map, header), 0);
        assert_eq!(core::mem::offset_of!(Map, n_entries), 8);
        assert_eq!(core::mem::offset_of!(Map, n_used), 12);
        assert_eq!(core::mem::offset_of!(Map, entries_cap), 16);
        assert_eq!(core::mem::offset_of!(Map, slots_count), 20);
        assert_eq!(core::mem::offset_of!(Map, n_tombstones), 24);
        assert_eq!(core::mem::offset_of!(Map, slots), 32);
        assert_eq!(core::mem::offset_of!(Map, entries), 40);
    }

    #[test]
    fn slot_pack_unpack_roundtrip() {
        let s = slot_make(0xdeadbeef, 0x12345678);
        assert_eq!(slot_hash(s), 0xdeadbeef);
        assert_eq!(slot_index(s), 0x12345678);
    }

    #[test]
    fn sentinels_disjoint() {
        assert_ne!(SLOT_EMPTY, SLOT_TOMBSTONE);
        assert_ne!(SLOT_EMPTY, 0);
        assert_ne!(SLOT_TOMBSTONE, 0);
    }
}
