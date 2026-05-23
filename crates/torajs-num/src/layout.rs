//! Layout constants for Str heap blocks — duplicated from
//! [`torajs-str::layout`] because torajs-num is a Layer-2 sibling
//! of torajs-str and architecture-rewrite.md forbids same-layer
//! dependencies (Layer-N → Layer-(N-1) only).
//!
//! The duplication cost is two `pub const` lines. These offsets
//! are ABI invariants pinned by `ssa_inkwell`-emitted GEPs at every
//! Str access site (see `torajs-str/src/layout.rs` for the full
//! invariant table). Drift between this file and torajs-str silently
//! corrupts every Number→Str path that reads Str input args.
//!
//! When a Layer-1 `torajs-types` crate eventually lands (post-P4),
//! these constants move there and both torajs-str + torajs-num
//! import from one source of truth.

/// Str payload length field — `*(u64*)(p + 8)`.
pub const STR_LEN_OFF: usize = 8;

/// Str payload data — `p + 16`.
pub const STR_DATA_OFF: usize = 16;
