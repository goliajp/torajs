//! Str block ABI constants + packed-header init + block-size
//! computation. Single source of truth for the byte layout the
//! toolchain emit layer (torajs-codegen) bakes into every Str
//! access site.
//!
//! ```text
//! Str = [header:8][length:4][_pad:4][bytes:N]   prefix 16
//!       ^             ^         ^      ^
//!       0             8        12     16    (offsets)
//! ```
//!
//! Header (8 bytes) layout matches [`torajs_rc::HeapHeader`]:
//! `refcount: u32 @0`, `type_tag: u16 @4`, `flags: u16 @6`. The
//! packed init writes all three fields in one `u64` store —
//! [`packed_header_init`] = `1 | (Tag::Str as u64) << 32 |
//! ((flags as u64) << 48)`.
//!
//! ## P11.1-S1 layout change (vs pre-S1 byte-Str)
//!
//! - `flags u16 @6` bit 1 now carries `IS_LATIN1` (1 = Latin-1
//!   payload, 0 = UTF-16 payload). `STATIC_LITERAL` keeps bit 0.
//! - `len u64 @8` is split into `length u32 @8 + _pad u32 @12`.
//!   `length` is the **code unit count** per ES spec
//!   (`String.length`): for Latin-1 = byte count; for UTF-16 = u16
//!   count. `_pad` is reserved for a future P11.x extension
//!   (hash slot / inline-cache hint / etc.).
//! - `byte_capacity` is **not stored**. Compute it from
//!   `(length, is_latin1)`: Latin-1 → `length`, UTF-16 →
//!   `length × 2`. Mirrors V8 SeqString / SpiderMonkey
//!   JSLinearString — neither stores capacity, both derive it.
//! - `STR_HDR_SIZE` = 16 and `STR_DATA_OFF` = 16 are unchanged so
//!   every emitted access against `STR_DATA_OFF` continues to
//!   resolve.
//! - S1 phase: every `alloc` path forces `is_latin1 = true`, so
//!   the payload semantics match the pre-S1 byte-Str exactly
//!   (Latin-1 0-255 ⊇ ASCII 0-127 ⊇ every existing fixture). UTF-16
//!   payload + build-time encoding decisions land in S2 / S3.

use torajs_rc::Tag;

/// Total bytes from `Str` block start to the first payload byte.
/// Toolchain-emitted Str data accesses bake in this constant.
pub const STR_HDR_SIZE: usize = 16;

/// Byte offset (from `Str` block start) of the `length` u32 field.
/// Toolchain-emitted Str length loads bake this in.
/// P11.1-S1: the field width shrank from u64 to
/// u32 here, but the offset stayed at 8 — only the load width at
/// the SSA / IR / runtime side changed.
pub const STR_LEN_OFF: usize = 8;

/// Byte offset (from `Str` block start) of the reserved u32 padding
/// slot. Currently unused; reserved for a future P11.x extension
/// (hash slot for `__torajs_str_eq` fast path, inline-cache hint,
/// etc.). Writers should leave it zero; readers must not assume any
/// specific value yet.
pub const STR_PAD_OFF: usize = 12;

/// Byte offset (from `Str` block start) of the first payload byte.
/// Always `STR_HDR_SIZE`; named separately so call sites read as
/// intent (`.add(STR_DATA_OFF)` is "advance past the prefix").
pub const STR_DATA_OFF: usize = STR_HDR_SIZE;

/// Max payload **byte** count eligible for the small-Str pool.
/// Strings whose `byte_capacity` ≤ this get rounded up to a uniform
/// pool block; larger strings go straight to `malloc`. Picked to
/// cover the dominant size class for split tokens, single-char
/// concat results, short-int `Number.toString` results, etc.
///
/// Note: this threshold is in **bytes**, not code units — Latin-1
/// short strings hold up to 16 code units in a pooled block,
/// UTF-16 short strings hold up to 8 code units in the same block.
pub const STR_POOL_PAYLOAD: u32 = 16;

/// Number of LIFO slots in the small-Str pool. Bounded so a
/// pathological "alloc 10 000 strings then drop them all" stays
/// bounded in memory; once full, additional drops fall through to
/// `free`. 32 covers tight `(s + t).drop()` loops without bloat.
pub const STR_POOL_SLOTS: usize = 32;

/// `flags u16 @6` bit 1 — set when the Str payload is encoded as
/// Latin-1 (ISO-8859-1, 0-255 code units, 1 byte each); clear when
/// the payload is UTF-16 little-endian (2 bytes per code unit, lone
/// surrogates permitted). Bit 0 is reserved for the pre-existing
/// `STATIC_LITERAL` flag in `torajs_rc`.
///
/// Defined here rather than in `torajs_rc` because the encoding
/// distinction is Str-specific — other heap types (Array, Obj,
/// Promise, ...) don't carry an encoding.
pub const STR_FLAG_IS_LATIN1: u16 = 0x0002;

/// Packed `u64` representation of a freshly-allocated Str header.
///
/// Byte layout (little-endian, all fields packed into one word):
///
/// | bits   | field    | value                       |
/// |--------|----------|-----------------------------|
/// | 0..32  | refcount | `1`                         |
/// | 32..48 | type_tag | `Tag::Str = 0`              |
/// | 48..64 | flags    | `IS_LATIN1` bit per `is_latin1` arg |
///
/// Writing this constant via `(p as *mut u64).write(...)` sets all
/// three fields in a single 8-byte store; subsequent allocator
/// stores only need to write the `length` u32 at offset
/// [`STR_LEN_OFF`] (and zero the reserved padding @
/// [`STR_PAD_OFF`] if the caller hasn't already).
///
/// Note: `Tag::Str` is repr(u16) = 0, but the cast to `u64` here
/// is safe — even if `Str` ever gets renumbered (it shouldn't, the
/// tag is wire-format), this expression still produces the
/// arithmetically correct packed word.
#[inline]
pub const fn packed_header_init(is_latin1: bool) -> u64 {
    let flags: u64 = if is_latin1 {
        STR_FLAG_IS_LATIN1 as u64
    } else {
        0
    };
    1u64 | ((Tag::Str as u64) << 32) | (flags << 48)
}

/// Block size (in bytes) that the allocator should request for a
/// Str holding `length` code units at the given `is_latin1`
/// encoding. Short strings get uniformly rounded up to
/// [`STR_POOL_PAYLOAD`] so every pooled block has the same
/// capacity (no risk of a small block being reused for a larger
/// payload). Anything past the pool cutoff pays the exact payload
/// size.
///
/// `byte_capacity = length × (if is_latin1 { 1 } else { 2 })`. Not
/// stored anywhere; recomputed here and at every drop / capacity
/// query — mirrors V8 SeqString sizing.
#[inline]
pub const fn block_size(length: u32, is_latin1: bool) -> usize {
    let byte_capacity = byte_capacity(length, is_latin1);
    let payload = if byte_capacity <= STR_POOL_PAYLOAD {
        STR_POOL_PAYLOAD
    } else {
        byte_capacity
    };
    STR_HDR_SIZE + payload as usize
}

/// Byte count that a payload of `length` code units occupies at
/// the given encoding. Computed; never stored. Inline-able to a
/// `lsl` on hot paths (UTF-16) or a no-op on Latin-1.
#[inline]
pub const fn byte_capacity(length: u32, is_latin1: bool) -> u32 {
    if is_latin1 { length } else { length * 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_header_layout_matches_c_latin1() {
        // refcount=1 in low 32 bits, type_tag=0 (Str) at [32:48],
        // flags=IS_LATIN1 at [48:64] when is_latin1=true. Mirrors
        // runtime_str.c __TORAJS_STR_HEADER_INIT post-S1.
        let w = packed_header_init(true);
        assert_eq!((w & 0xFFFF_FFFF) as u32, 1, "refcount");
        assert_eq!(((w >> 32) & 0xFFFF) as u16, Tag::Str as u16, "tag");
        assert_eq!(
            ((w >> 48) & 0xFFFF) as u16,
            STR_FLAG_IS_LATIN1,
            "flags carry IS_LATIN1"
        );
    }

    #[test]
    fn packed_header_layout_matches_c_utf16() {
        // is_latin1=false → flags=0 (no IS_LATIN1 bit).
        let w = packed_header_init(false);
        assert_eq!((w & 0xFFFF_FFFF) as u32, 1, "refcount");
        assert_eq!(((w >> 32) & 0xFFFF) as u16, Tag::Str as u16, "tag");
        assert_eq!(((w >> 48) & 0xFFFF) as u16, 0, "flags clear");
    }

    #[test]
    fn block_size_short_latin1_round_up_to_pool_payload() {
        assert_eq!(block_size(0, true), STR_HDR_SIZE + 16);
        assert_eq!(block_size(1, true), STR_HDR_SIZE + 16);
        assert_eq!(block_size(15, true), STR_HDR_SIZE + 16);
        assert_eq!(block_size(16, true), STR_HDR_SIZE + 16);
    }

    #[test]
    fn block_size_long_latin1_pay_exact_payload() {
        assert_eq!(block_size(17, true), STR_HDR_SIZE + 17);
        assert_eq!(block_size(100, true), STR_HDR_SIZE + 100);
        assert_eq!(block_size(1024, true), STR_HDR_SIZE + 1024);
    }

    #[test]
    fn block_size_utf16_doubles_byte_capacity() {
        // 8 UTF-16 code units = 16 bytes = pool slot.
        assert_eq!(block_size(8, false), STR_HDR_SIZE + 16);
        // 9 code units = 18 bytes = past pool cutoff.
        assert_eq!(block_size(9, false), STR_HDR_SIZE + 18);
        assert_eq!(block_size(100, false), STR_HDR_SIZE + 200);
    }

    #[test]
    fn byte_capacity_matches_encoding() {
        assert_eq!(byte_capacity(0, true), 0);
        assert_eq!(byte_capacity(16, true), 16);
        assert_eq!(byte_capacity(0, false), 0);
        assert_eq!(byte_capacity(16, false), 32);
    }

    #[test]
    fn layout_offsets_match_c_macros() {
        assert_eq!(STR_HDR_SIZE, 16);
        assert_eq!(STR_LEN_OFF, 8);
        assert_eq!(STR_PAD_OFF, 12);
        assert_eq!(STR_DATA_OFF, 16);
        assert_eq!(STR_POOL_PAYLOAD, 16);
        assert_eq!(STR_POOL_SLOTS, 32);
        assert_eq!(STR_FLAG_IS_LATIN1, 0x0002);
    }
}
