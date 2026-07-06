//! Bacon-Rajan cycle-collector trial-deletion color state.
//!
//! Extracted from `lib.rs` to keep that file under the 500-prod-LOC
//! file-size hard limit (`rules/common/file-size.md`). The
//! `HeapHeader::color()` / `set_color()` methods + the Color
//! roundtrip tests stay in `lib.rs` since they operate on
//! `HeapHeader.flags`; this module owns the enum + bit-field
//! positioning constants and re-exports through the crate root.

/// Bit position of the 2-bit cycle-collector color field.
///
/// Moved from 3 to 13 (RFC 20260706 chunk 573): the old bits 3-4
/// overlapped `FLAG_ARR_ANY` / `FLAG_FROZEN` behind a "use sites
/// are disjoint" assumption that did NOT hold for FROZEN — the
/// collector colors declared-class instances, and `Object.freeze`
/// marks those too, so buffering a frozen obj Purple (bit 4) and
/// scanning it back Black cleared the freeze marker (probe: store
/// after `Bun.gc` wrote through, bun throws). Bits 13-14 are free
/// across every tag (bit 15 stays spare).
pub const COLOR_SHIFT: u16 = 13;
/// Mask covering both color bits.
pub const COLOR_MASK: u16 = 0b11 << COLOR_SHIFT;

/// Bacon-Rajan trial-deletion state. Stored in
/// `HeapHeader::flags` at bits 13-14 (see [`COLOR_SHIFT`] /
/// [`COLOR_MASK`]) — disjoint from every flag user.
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
