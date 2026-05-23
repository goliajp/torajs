//! BigInt heap layout mirror constants.
//!
//! Bit-for-bit equivalent of `runtime_bigint.c`'s `BigIntHeader`
//! struct. Sign-and-magnitude with u64-limb little-endian magnitude:
//!
//! ```text
//! offset | size | field
//! -------|------|------
//!   0    |  8B  | universal heap header (refcount + type_tag + flags)
//!   8    |  4B  | sign (0 = non-negative, 1 = negative)
//!  12    |  4B  | len  (number of u64 limbs; 0 = canonical zero)
//!  16    | 8×n  | words[len] — words[0] = least significant 2^0..2^64
//! ```
//!
//! **Canonical invariant** (every constructor must maintain):
//! - `words[len - 1] != 0` — no leading zero limbs
//! - if `len == 0` then `sign == 0` — no signed zero
//!
//! These offsets are duplicated here (not pulled from a shared
//! Layer-1 crate) per the [[feedback-narrow-abi-surface]] pattern:
//! Layer-2 siblings forbid Cargo deps to each other; cross-tier
//! handoff uses C-ABI symbol resolution at link time.

/// Universal heap header size (`{ refcount: u32, type_tag: u16, flags: u16 }`).
/// Defined by `torajs-rc::HeapHeader`; duplicated here as a constant
/// so the layout math is reviewable without crossing crate boundaries.
pub const HEAP_HEADER_SIZE: usize = 8;

/// Offset of the `sign` u32 within the heap block.
pub const SIGN_OFF: usize = HEAP_HEADER_SIZE;

/// Offset of the `len` u32 within the heap block.
pub const LEN_OFF: usize = HEAP_HEADER_SIZE + 4;

/// Offset of `words[0]` — the inline-following limb array.
pub const WORDS_OFF: usize = HEAP_HEADER_SIZE + 8;

/// The `type_tag` value for BigInt heap values (matches
/// `runtime_bigint.c`'s `__TORAJS_TAG_BIGINT`).
pub const TAG_BIGINT: u16 = 10;

/// Mirror of `torajs-str::layout::STR_HDR_SIZE` (= 16). Used by
/// construct fns that receive a Str pointer (`__torajs_bigint_from_decimal`
/// / `_from_hex` / `_from_str`) — body bytes start at `s + STR_HDR_SIZE`,
/// `len` at `s + STR_LEN_OFF` (offset 8). Duplicated as constant here
/// (not Cargo-dep'd from torajs-str) per the same-layer cross-tier
/// extern pattern: Layer-2 siblings forbid mutual deps; cross-tier
/// data layouts replicate constants on each side.
pub const STR_HDR_SIZE: usize = 16;

/// Mirror of `torajs-str::layout::STR_LEN_OFF` (= 8). Offset of the
/// Str's `len` u64 within its heap block.
pub const STR_LEN_OFF: usize = 8;
