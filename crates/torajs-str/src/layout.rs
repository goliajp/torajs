//! Str block ABI constants + packed-header init + block-size
//! computation. Single source of truth for the byte layout
//! `ssa_inkwell` GEPs against and the runtime_*.c macros (`__TORAJS_
//! STR_LEN(p)` etc.) mirror.
//!
//! ```text
//! Str = [header:8][len:8][bytes:N]   prefix 16
//!       ^             ^      ^
//!       0             8      16    (offsets)
//! ```
//!
//! Header (8 bytes) layout matches [`torajs_rc::HeapHeader`]:
//! `refcount: u32 @0`, `type_tag: u16 @4`, `flags: u16 @6`. The
//! packed init writes all three fields in one `u64` store —
//! [`packed_header_init`] = `1 | (Tag::Str as u64) << 32`.

use torajs_rc::Tag;

/// Total bytes from `Str` block start to the first payload byte.
/// `ssa_inkwell::emit_str_data_gep` GEPs against this constant; the
/// runtime_str.c `__TORAJS_STR_HDR_SIZE` mirrors it.
pub const STR_HDR_SIZE: usize = 16;

/// Byte offset (from `Str` block start) of the `len` u64 field.
/// `ssa_inkwell::emit_str_len_gep` + `__TORAJS_STR_LEN(p)` C macro
/// both mirror this.
pub const STR_LEN_OFF: usize = 8;

/// Byte offset (from `Str` block start) of the first payload byte.
/// Always `STR_HDR_SIZE`; named separately so call sites read as
/// intent (`.add(STR_DATA_OFF)` is "advance past the prefix").
pub const STR_DATA_OFF: usize = STR_HDR_SIZE;

/// Max payload length (in bytes) eligible for the small-Str pool.
/// Strings ≤ this size get rounded up to a uniform pool block;
/// larger strings go straight to `malloc`. Picked to cover the
/// dominant size class for split tokens, single-char concat
/// results, short-int `Number.toString` results, etc.
pub const STR_POOL_PAYLOAD: u64 = 16;

/// Number of LIFO slots in the small-Str pool. Bounded so a
/// pathological "alloc 10 000 strings then drop them all" stays
/// bounded in memory; once full, additional drops fall through to
/// `free`. 32 covers tight `(s + t).drop()` loops without bloat.
pub const STR_POOL_SLOTS: usize = 32;

/// Packed `u64` representation of a freshly-allocated Str header.
///
/// Byte layout (little-endian, all fields packed into one word):
///
/// | bits   | field    | value          |
/// |--------|----------|----------------|
/// | 0..32  | refcount | `1`            |
/// | 32..48 | type_tag | `Tag::Str = 0` |
/// | 48..64 | flags    | `0`            |
///
/// Writing this constant via `(p as *mut u64).write(...)` sets all
/// three fields in a single 8-byte store; subsequent allocator
/// stores only need to write the `len` u64 at offset
/// [`STR_LEN_OFF`].
///
/// Note: `Tag::Str` is repr(u16) = 0, but the cast to `u64` here
/// is safe — even if `Str` ever gets renumbered (it shouldn't, the
/// tag is wire-format), this expression still produces the
/// arithmetically correct packed word.
#[inline]
pub const fn packed_header_init() -> u64 {
    1u64 | ((Tag::Str as u64) << 32)
}

/// Block size (in bytes) that the allocator should request for a
/// Str holding `len` payload bytes. Short strings get uniformly
/// rounded up to [`STR_POOL_PAYLOAD`] so every pooled block has
/// the same capacity (no risk of a small block being reused for a
/// larger payload). Anything past the pool cutoff pays the exact
/// payload size.
#[inline]
pub const fn block_size(len: u64) -> usize {
    let payload = if len <= STR_POOL_PAYLOAD {
        STR_POOL_PAYLOAD
    } else {
        len
    };
    STR_HDR_SIZE + payload as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_header_layout_matches_c() {
        // refcount=1 in low 32 bits, type_tag=0 (Str) at [32:48],
        // flags=0 at [48:64]. Mirrors runtime_str.c
        // __TORAJS_STR_HEADER_INIT.
        assert_eq!(packed_header_init(), 1u64 | (0u64 << 32));
        // Sanity: the low 32 bits decode as refcount=1, upper 32
        // are all zero (tag=Str=0, flags=0).
        let w = packed_header_init();
        assert_eq!((w & 0xFFFF_FFFF) as u32, 1, "refcount");
        assert_eq!(((w >> 32) & 0xFFFF) as u16, Tag::Str as u16, "tag");
        assert_eq!(((w >> 48) & 0xFFFF) as u16, 0, "flags");
    }

    #[test]
    fn block_size_short_strings_round_up_to_pool_payload() {
        assert_eq!(block_size(0), STR_HDR_SIZE + 16);
        assert_eq!(block_size(1), STR_HDR_SIZE + 16);
        assert_eq!(block_size(15), STR_HDR_SIZE + 16);
        assert_eq!(block_size(16), STR_HDR_SIZE + 16);
    }

    #[test]
    fn block_size_long_strings_pay_exact_payload() {
        assert_eq!(block_size(17), STR_HDR_SIZE + 17);
        assert_eq!(block_size(100), STR_HDR_SIZE + 100);
        assert_eq!(block_size(1024), STR_HDR_SIZE + 1024);
    }

    #[test]
    fn layout_offsets_match_c_macros() {
        assert_eq!(STR_HDR_SIZE, 16);
        assert_eq!(STR_LEN_OFF, 8);
        assert_eq!(STR_DATA_OFF, 16);
        assert_eq!(STR_POOL_PAYLOAD, 16);
        assert_eq!(STR_POOL_SLOTS, 32);
    }
}
