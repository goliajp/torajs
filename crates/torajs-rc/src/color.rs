//! Bacon-Rajan cycle-collector trial-deletion color state.
//!
//! Extracted from `lib.rs` to keep that file under the 500-prod-LOC
//! file-size hard limit (`rules/common/file-size.md`). The
//! `HeapHeader::color()` / `set_color()` methods + the Color
//! roundtrip tests stay in `lib.rs` since they operate on
//! `HeapHeader.flags`; this module owns the enum + bit-field
//! positioning constants and re-exports through the crate root.

/// Bit position of the 2-bit cycle-collector color field.
pub const COLOR_SHIFT: u16 = 3;
/// Mask covering both color bits.
pub const COLOR_MASK: u16 = 0b11 << COLOR_SHIFT;

/// Bacon-Rajan trial-deletion state. Stored in
/// `HeapHeader::flags` at bits 3-4 (see [`COLOR_SHIFT`] /
/// [`COLOR_MASK`]).
///
/// Note the bit overlap with `FLAG_ARR_ANY` / `FLAG_FROZEN`:
/// cycle-collector traversal only runs on container types
/// (`Tag::Obj` / `Tag::Arr`); ARR_ANY only marks Array<Any>;
/// FROZEN only applies to plain objects. The use sites are
/// disjoint, so the same bits can serve both readers safely —
/// don't repurpose either without an audit of every cycle /
/// freeze / Array<Any> code path.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// In use; no cycle suspicion.
    Black = 0 << COLOR_SHIFT,
    /// Being marked during the current trial-deletion pass.
    Gray = 1 << COLOR_SHIFT,
    /// Buffered as a potential cycle root.
    Purple = 2 << COLOR_SHIFT,
    /// Confirmed garbage; freed by the collect phase.
    White = 3 << COLOR_SHIFT,
}
