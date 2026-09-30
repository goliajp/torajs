//! Layout constants for Str heap blocks — duplicated from
//! [`torajs-str::layout`] because torajs-num is a Layer-2 sibling
//! of torajs-str and the architecture rewrite forbids same-layer
//! dependencies (Layer-N → Layer-(N-1) only).
//!
//! The duplication cost is two `pub const` lines. These offsets
//! are ABI invariants baked into every toolchain-emitted Str
//! access site (see `torajs-str/src/layout.rs` for the full
//! invariant table). Drift between this file and torajs-str silently
//! corrupts every Number→Str path that reads Str input args.
//!
//! When a Layer-1 `torajs-types` crate eventually lands (post-P4),
//! these constants move there and both torajs-str + torajs-num
//! import from one source of truth.

/// Str payload length field — `*(u32*)(p + 8)` (code unit count;
/// `_pad u32 @12` is reserved-zero, so a legacy u64 read happens to
/// yield the same value).
pub const STR_LEN_OFF: usize = 8;

/// Str payload data — `p + 16`.
pub const STR_DATA_OFF: usize = 16;

/// HeapHeader `flags: u16` field — `*(u16*)(p + 6)`.
pub const STR_FLAGS_OFF: usize = 6;

/// `flags` bit 1 — payload encoding: 1 = Latin-1 (1 byte / code
/// unit), 0 = UTF-16 LE (2 bytes / code unit).
pub const STR_FLAG_IS_LATIN1: u16 = 0x0002;

/// `flags` bit 10 — the block is a Substr view (`torajs-str`
/// `FLAG_SUBSTR_VIEW`): `len u64 @8` code units of the parent Str at
/// `*(ptr*)(p + 16)`, starting `*(u64*)(p + 24)` code units in. The
/// parent carries the encoding.
pub const FLAG_SUBSTR_VIEW: u16 = 1 << 10;
pub const SUBSTR_PARENT_OFF: usize = 16;
pub const SUBSTR_OFFSET_OFF: usize = 24;
