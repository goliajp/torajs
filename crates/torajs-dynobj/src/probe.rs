//! Hash-index probe + key hash / equality + block field accessors.
//!
//! Pure-Rust internals shared by [`crate::get`] / [`crate::set`] /
//! [`crate::define`] / [`crate::has`] / [`crate::delete`] /
//! [`crate::drop`] / [`crate::iter`] / [`crate::resize`].
//!
//! The block is a compact insertion-ordered dict (see [`crate::layout`]):
//! `index[cap]` u32 slots map probe positions to dense-entry indices;
//! `entries[entries_cap]` holds `{ key_ptr_tagged, value_anyv }` pairs
//! in insertion order.
//!
//! Probe contract: linear step = 1; mask = `cap - 1` (cap is power of
//! 2); [`IDX_TOMBSTONE`] slots are walked past but remembered as the
//! first insertion candidate (lazy slot reuse on next insert).
//!
//! A key is either §6.1.7 kind — Str or Symbol — and [`hash_key`] /
//! [`key_eq`] / [`drop_key`] dispatch on the key cell's own tag (see
//! [`crate::layout`]'s key-domain note). This module owns that
//! dispatch so no consumer re-derives it.

use core::ffi::c_void;

use crate::layout::{
    BUCKET_FLAGS_MASK, BUCKET_KEY_PTR_MASK, DYNOBJ_CAP_OFF, DYNOBJ_COUNT_OFF,
    DYNOBJ_ENTRIES_CAP_OFF, DYNOBJ_ENTRIES_LEN_OFF, DYNOBJ_STORE_OFF, IDX_EMPTY, IDX_TOMBSTONE,
    STR_DATA_OFF, STR_LEN_OFF, TAG_SYMBOL_KEY,
};

unsafe extern "C" {
    /// Cross-tier — torajs-str's Str drop (an entry's owning
    /// string-key share).
    fn __torajs_str_drop(s: *mut c_void);
    /// Cross-tier — torajs-str's Symbol drop (an entry's owning
    /// symbol-key share).
    fn __torajs_symbol_drop(s: *mut c_void);
}

/// Dense-array entry — 16 bytes, `#[repr(C)]`. `key_ptr_tagged` encodes
/// the key-cell pointer (bits 3+) plus the W/E/C PropertyDescriptor flags
/// (bits 0/1/2), or [`crate::layout::DYNOBJ_KEY_HOLE`] for a deleted
/// hole; `value_anyv` is a NaN-box AnyValue carrying the slot's
/// (tag, value) pair.
#[repr(C)]
pub(crate) struct Entry {
    pub(crate) key_ptr_tagged: u64,
    pub(crate) value_anyv: u64,
}

/// Decode the real key-cell pointer (Str or Symbol) from a
/// `key_ptr_tagged` word.
#[inline]
pub(crate) fn bucket_key_ptr(tagged: u64) -> *mut c_void {
    (tagged & BUCKET_KEY_PTR_MASK) as *mut c_void
}

/// Extract the W/E/C flag bits from a `key_ptr_tagged` word.
#[inline]
pub(crate) fn bucket_flags(tagged: u64) -> u64 {
    tagged & BUCKET_FLAGS_MASK
}

/// Re-pack a `(ptr, flags)` pair into a fresh `key_ptr_tagged` word.
/// `flags` is masked to its low 3 bits; `ptr` must be 8-aligned (both
/// torajs-str key kinds satisfy this — Str blocks and the 16-byte,
/// align-8 Symbol cell).
#[inline]
pub(crate) fn bucket_make_key_tagged(ptr: *mut c_void, flags: u64) -> u64 {
    (ptr as u64) | (flags & BUCKET_FLAGS_MASK)
}

/// Read the dynobj's `count: u32` (live entries, holes excluded).
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn count(obj: *const c_void) -> u32 {
    unsafe { *((obj as *const u8).add(DYNOBJ_COUNT_OFF) as *const u32) }
}

/// Write the dynobj's `count: u32`.
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn set_count(obj: *mut c_void, v: u32) {
    unsafe { *((obj as *mut u8).add(DYNOBJ_COUNT_OFF) as *mut u32) = v }
}

/// Read the dynobj's `cap: u32` (hash-index slot count).
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn cap(obj: *const c_void) -> u32 {
    unsafe { *((obj as *const u8).add(DYNOBJ_CAP_OFF) as *const u32) }
}

/// Read the dynobj's `entries_len: u32` (dense-array used length,
/// holes included — the iteration upper bound).
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn entries_len(obj: *const c_void) -> u32 {
    unsafe { *((obj as *const u8).add(DYNOBJ_ENTRIES_LEN_OFF) as *const u32) }
}

/// Write the dynobj's `entries_len: u32`.
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn set_entries_len(obj: *mut c_void, v: u32) {
    unsafe { *((obj as *mut u8).add(DYNOBJ_ENTRIES_LEN_OFF) as *mut u32) = v }
}

/// Read the dynobj's `entries_cap: u32` (dense-array capacity).
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn entries_cap(obj: *const c_void) -> u32 {
    unsafe { *((obj as *const u8).add(DYNOBJ_ENTRIES_CAP_OFF) as *const u32) }
}

/// Read the dynobj's `store: *mut u8` (the index+entries block).
///
/// # Safety
/// `obj` must point at a live dynobj header cell.
#[inline]
pub(crate) unsafe fn store_ptr(obj: *const c_void) -> *mut u8 {
    unsafe { *((obj as *const u8).add(DYNOBJ_STORE_OFF) as *const *mut u8) }
}

/// Write the dynobj's `store: *mut u8` — [`crate::alloc`]'s fresh
/// wiring and [`crate::resize`]'s swap are the only writers.
///
/// # Safety
/// `obj` must point at a live dynobj header cell.
#[inline]
pub(crate) unsafe fn set_store_ptr(obj: *mut c_void, p: *mut u8) {
    unsafe { *((obj as *mut u8).add(DYNOBJ_STORE_OFF) as *mut *mut u8) = p }
}

/// Pointer to the start of the `index[cap]` u32 array (store offset 0).
///
/// # Safety
/// `obj` must point at a live dynobj header cell with a live store.
#[inline]
pub(crate) unsafe fn index_ptr(obj: *const c_void) -> *mut u32 {
    unsafe { store_ptr(obj) as *mut u32 }
}

/// Pointer to the start of the dense entry array (sits after the
/// index within the store, so the offset depends on `cap`).
///
/// # Safety
/// `obj` must point at a live dynobj header cell with a live store.
#[inline]
pub(crate) unsafe fn entries(obj: *const c_void) -> *mut Entry {
    let cap = unsafe { cap(obj) };
    unsafe { store_ptr(obj).add(cap as usize * 4) as *mut Entry }
}

/// Read a Str's `length: u32` (offset 8). The four bytes above it
/// are the capacity slot, not part of the length.
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
unsafe fn str_len(key: *const c_void) -> u64 {
    unsafe { *((key as *const u8).add(STR_LEN_OFF) as *const u32) as u64 }
}

/// Pointer to a Str's inline payload (offset 16) — Latin-1 bytes or
/// UTF-16 LE code units, per [`str_is_latin1`].
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
unsafe fn str_data(key: *const c_void) -> *const u8 {
    unsafe { (key as *const u8).add(STR_DATA_OFF) }
}

/// Universal heap-header `flags u16 @6` and the torajs-str
/// `IS_LATIN1` bit in it (1 = one byte per code unit, 0 = UTF-16).
pub(crate) const KEY_HDR_FLAGS_OFF: usize = 6;
pub(crate) const KEY_STR_FLAG_IS_LATIN1: u16 = 0x0002;

/// The key's payload encoding.
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
pub(crate) unsafe fn str_is_latin1(key: *const c_void) -> bool {
    let flags = unsafe { *((key as *const u8).add(KEY_HDR_FLAGS_OFF) as *const u16) };
    flags & KEY_STR_FLAG_IS_LATIN1 != 0
}

/// The key's payload as bytes — `length` bytes when Latin-1, twice
/// that when UTF-16. Every walk over a key's content goes through
/// here: `length` counts code units, not bytes.
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
unsafe fn str_payload<'a>(key: *const c_void) -> &'a [u8] {
    let len = unsafe { str_len(key) } as usize;
    let n = if unsafe { str_is_latin1(key) } {
        len
    } else {
        len * 2
    };
    unsafe { core::slice::from_raw_parts(str_data(key), n) }
}

/// True when a key cell is a Symbol rather than a Str — the §6.1.7
/// property-key domain split, read off the cell's own universal
/// heap-header `type_tag` (offset 4).
///
/// # Safety
/// `key` must point at a live heap block with a universal header.
#[inline]
pub(crate) unsafe fn key_is_symbol(key: *const c_void) -> bool {
    unsafe { *((key as *const u8).add(4) as *const u16) == TAG_SYMBOL_KEY }
}

/// A key's Str payload as `(data, byte_len, is_latin1)`, or `None`
/// when the key is a Symbol — the §6.1.7 domain split applied at the
/// one place the payload is reachable. `byte_len` already carries the
/// encoding stride; a consumer comparing against an ASCII spelling
/// must also require `is_latin1` (a UTF-16 payload can spell the
/// same bytes with different units).
///
/// The two cells overlap where it hurts: a Str keeps its `len: u64`
/// at offset 8, a Symbol keeps its *description pointer* there. Read
/// blind, a Symbol key hands out a heap address as a byte count, and
/// whatever walks that span runs off the end of the mapping. Every
/// consumer that wants the name behind a key goes through here, so
/// the gate cannot be present on one arm of a pair and missing on
/// the other.
///
/// # Safety
/// `key` must point at a live Str or Symbol heap block.
#[inline]
pub(crate) unsafe fn key_str_bytes(key: *const c_void) -> Option<(*const u8, u64, bool)> {
    if unsafe { key_is_symbol(key) } {
        return None;
    }
    let payload = unsafe { str_payload(key) };
    Some((payload.as_ptr(), payload.len() as u64, unsafe {
        str_is_latin1(key)
    }))
}

/// Release an entry's owning key share, dispatching on the §6.1.7 key
/// kind. Single site so the delete and whole-block drop walks cannot
/// drift on which dropper a Symbol key needs.
///
/// # Safety
/// `key` must point at a live Str or Symbol heap block that this
/// dynobj owns a `+1` share of. The pointee may be freed on return.
#[inline]
pub(crate) unsafe fn drop_key(key: *mut c_void) {
    if unsafe { key_is_symbol(key) } {
        unsafe { __torajs_symbol_drop(key) }
    } else {
        unsafe { __torajs_str_drop(key) }
    }
}

/// FNV-1a hash over the Str's payload bytes (64-bit constants). The
/// walk covers the whole payload — `length` code units, one byte
/// each for Latin-1 and two for UTF-16 — so two UTF-16 keys that
/// agree on their low bytes (`"\uD800"` / `"\uDC00"`) hash apart.
/// Under torajs-str's canonical-encoding invariant equal content
/// means equal encoding, so the byte walk is a content hash; an
/// ASCII key's hash is the FNV-1a of its ASCII bytes
/// (`attach_exec.rs` bakes those at compile time).
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
pub(crate) unsafe fn hash_str(key: *const c_void) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in unsafe { str_payload(key) } {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// FNV-1a hash over a Symbol key's cell address. A Symbol's identity
/// is its cell (§20.4 — description is not identity), so the pointer
/// *is* the hash input; running it through the same FNV-1a the string
/// lane uses spreads the always-zero low alignment bits, which a bare
/// `ptr & mask` probe start would bunch into every 16th slot.
///
/// # Safety
/// `key` must point at a live Symbol heap block.
#[inline]
pub(crate) unsafe fn hash_symbol(key: *const c_void) -> u64 {
    let bytes = (key as u64).to_le_bytes();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Hash a property key of either §6.1.7 kind — content for a Str,
/// cell identity for a Symbol.
///
/// # Safety
/// `key` must point at a live Str or Symbol heap block.
#[inline]
pub(crate) unsafe fn hash_key(key: *const c_void) -> u64 {
    if unsafe { key_is_symbol(key) } {
        unsafe { hash_symbol(key) }
    } else {
        unsafe { hash_str(key) }
    }
}

/// Property-key equality across both §6.1.7 kinds. Same-kind keys
/// compare by their own rule (Str content / Symbol cell identity);
/// a Str and a Symbol are never the same key.
///
/// # Safety
/// `a` and `b` must each point at a live Str or Symbol heap block.
#[inline]
pub(crate) unsafe fn key_eq(a: *const c_void, b: *const c_void) -> bool {
    if a == b {
        return true;
    }
    // A Symbol key can only equal the very same cell, so any surviving
    // Symbol on either side is already a miss.
    if unsafe { key_is_symbol(a) } || unsafe { key_is_symbol(b) } {
        return false;
    }
    unsafe { str_eq(a, b) }
}

/// Compare two Str values for equality: same encoding, same payload
/// bytes (the whole payload, see [`hash_str`]). Equal content under
/// the canonical-encoding invariant is exactly that; the encoding
/// test keeps a Latin-1 `"AB"` apart from a UTF-16 `"\u4241"`, whose
/// payload bytes coincide. Pointer-identity short-circuit for
/// interned literals.
///
/// # Safety
/// `a` and `b` must each point at a live Str heap block.
#[inline]
pub(crate) unsafe fn str_eq(a: *const c_void, b: *const c_void) -> bool {
    if a == b {
        return true;
    }
    unsafe { str_is_latin1(a) == str_is_latin1(b) && str_payload(a) == str_payload(b) }
}

/// Verdict from a [`probe`] walk.
pub(crate) struct Probe {
    /// Hash-index slot — if `found`, the live key's slot; if not,
    /// the insertion target (first tombstone, else first empty).
    pub slot: u32,
    /// Dense-entry index — meaningful only when `found`.
    pub entry: u32,
    /// True iff `key` is present in the table.
    pub found: bool,
}

/// Walk the hash index looking for `key`. Linear probe step = 1.
/// First reachable [`IDX_EMPTY`] slot terminates the walk; first
/// [`IDX_TOMBSTONE`] is remembered for insert reuse.
///
/// # Safety
/// `obj` must point at a live dynobj heap block; `key` at a live Str
/// or Symbol cell.
pub(crate) unsafe fn probe(obj: *const c_void, key: *const c_void) -> Probe {
    let cap = unsafe { cap(obj) };
    let idx = unsafe { index_ptr(obj) };
    let ent = unsafe { entries(obj) };
    let h = unsafe { hash_key(key) };
    let mask = cap - 1;
    let start = (h as u32) & mask;
    let mut tombstone_at: Option<u32> = None;
    for step in 0..cap {
        let slot = (start + step) & mask;
        let iv = unsafe { *idx.add(slot as usize) };
        if iv == IDX_EMPTY {
            return Probe {
                slot: tombstone_at.unwrap_or(slot),
                entry: 0,
                found: false,
            };
        }
        if iv == IDX_TOMBSTONE {
            if tombstone_at.is_none() {
                tombstone_at = Some(slot);
            }
            continue;
        }
        let kp_tagged = unsafe { (*ent.add(iv as usize)).key_ptr_tagged };
        if unsafe { key_eq(bucket_key_ptr(kp_tagged) as *const c_void, key) } {
            return Probe {
                slot,
                entry: iv,
                found: true,
            };
        }
    }
    // Unreachable in practice: entries_cap = cap * 7/8 keeps at least
    // cap / 8 index slots empty.
    Probe {
        slot: tombstone_at.unwrap_or(0),
        entry: 0,
        found: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{DYNOBJ_KEY_HOLE, entries_cap_for};

    #[test]
    fn entry_layout() {
        assert_eq!(core::mem::size_of::<Entry>(), 16);
        assert_eq!(core::mem::align_of::<Entry>(), 8);
        assert_eq!(core::mem::offset_of!(Entry, key_ptr_tagged), 0);
        assert_eq!(core::mem::offset_of!(Entry, value_anyv), 8);
    }

    /// `bucket_key_ptr` / `bucket_flags` round-trip — pack then
    /// decompose recovers both halves. Plus the hole sentinel stays
    /// disjoint from any live packed key.
    #[test]
    fn key_ptr_flag_pack_round_trip() {
        let fake_ptr = 0x1234_5678_0000_0040u64 as *mut c_void;
        let flags = 0b101u64; // W + C, not E
        let tagged = bucket_make_key_tagged(fake_ptr, flags);
        assert_eq!(tagged, 0x1234_5678_0000_0045u64);
        assert_eq!(bucket_key_ptr(tagged), fake_ptr);
        assert_eq!(bucket_flags(tagged), flags);

        // Hole sentinel (0) only meets ptr=0 — disjoint from live keys.
        assert_eq!(DYNOBJ_KEY_HOLE, 0);
        assert_ne!(bucket_make_key_tagged(fake_ptr, BUCKET_FLAGS_MASK), 0);
    }

    /// Index sentinels stay disjoint from any representable entry
    /// index (entries_cap maxes out well below u32::MAX - 1).
    #[test]
    fn index_sentinels_disjoint() {
        assert_eq!(IDX_EMPTY, u32::MAX);
        assert_eq!(IDX_TOMBSTONE, u32::MAX - 1);
        assert!(entries_cap_for(1 << 30) < IDX_TOMBSTONE);
    }

    /// A Str-shaped block on the heap so the layout reads land in
    /// valid memory: [hdr:8][len:8][payload]. `units` counts code
    /// units; the header carries the `IS_LATIN1` bit when asked.
    fn make_key(payload: &[u8], units: u64, latin1: bool) -> Vec<u8> {
        let mut v = vec![0u8; STR_DATA_OFF + payload.len()];
        if latin1 {
            v[KEY_HDR_FLAGS_OFF..KEY_HDR_FLAGS_OFF + 2]
                .copy_from_slice(&KEY_STR_FLAG_IS_LATIN1.to_le_bytes());
        }
        v[STR_LEN_OFF..STR_LEN_OFF + 8].copy_from_slice(&units.to_le_bytes());
        v[STR_DATA_OFF..].copy_from_slice(payload);
        v
    }

    fn latin1(s: &str) -> Vec<u8> {
        make_key(s.as_bytes(), s.len() as u64, true)
    }

    fn utf16(units: &[u16]) -> Vec<u8> {
        let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        make_key(&bytes, units.len() as u64, false)
    }

    /// FNV-1a known-answer: hash of empty string = offset basis.
    #[test]
    fn hash_str_empty_is_fnv_offset_basis() {
        let buf = latin1("");
        let p = buf.as_ptr() as *const c_void;
        assert_eq!(unsafe { hash_str(p) }, 0xcbf29ce484222325);
    }

    /// FNV-1a known-answer: hash of `"a"` (single byte 0x61).
    #[test]
    fn hash_str_single_byte_a() {
        let buf = latin1("a");
        let p = buf.as_ptr() as *const c_void;
        // 0xcbf29ce484222325 ^ 0x61 = 0xcbf29ce484222344, then * 0x100000001b3
        let expected = (0xcbf29ce484222325u64 ^ 0x61u64).wrapping_mul(0x100000001b3);
        assert_eq!(unsafe { hash_str(p) }, expected);
    }

    /// The hash walks the whole UTF-16 payload: two lone surrogates
    /// that share a low byte hash apart, and so do `"\u0100"` /
    /// `"\u0200"`.
    #[test]
    fn hash_str_covers_both_bytes_of_a_utf16_unit() {
        let hi = utf16(&[0xD800]);
        let lo = utf16(&[0xDC00]);
        let a = utf16(&[0x0100]);
        let b = utf16(&[0x0200]);
        let h = |v: &Vec<u8>| unsafe { hash_str(v.as_ptr() as *const c_void) };
        assert_ne!(h(&hi), h(&lo));
        assert_ne!(h(&a), h(&b));
    }

    /// str_eq: identical pointer short-circuit; equal-bytes match;
    /// different-len reject; equal-len different-bytes reject; a
    /// UTF-16 pair is compared over both bytes of every unit; equal
    /// payload bytes under different encodings are different keys.
    #[test]
    fn str_eq_cases() {
        let a = latin1("hello");
        let b = latin1("hello");
        let c = latin1("world");
        let d = latin1("hi");
        let p = |v: &Vec<u8>| v.as_ptr() as *const c_void;
        assert!(unsafe { str_eq(p(&a), p(&a)) }, "identity");
        assert!(unsafe { str_eq(p(&a), p(&b)) }, "equal bytes");
        assert!(
            !unsafe { str_eq(p(&a), p(&c)) },
            "different bytes, same len"
        );
        assert!(!unsafe { str_eq(p(&a), p(&d)) }, "different lens");
        let hi = utf16(&[0xD800]);
        let hi2 = utf16(&[0xD800]);
        let lo = utf16(&[0xDC00]);
        assert!(unsafe { str_eq(p(&hi), p(&hi2)) }, "same UTF-16 unit");
        assert!(
            !unsafe { str_eq(p(&hi), p(&lo)) },
            "units differ in the high byte"
        );
        let ab = latin1("AB");
        let wide = utf16(&[0x4241]);
        assert!(
            !unsafe { str_eq(p(&ab), p(&wide)) },
            "same bytes, different encoding"
        );
    }
}
