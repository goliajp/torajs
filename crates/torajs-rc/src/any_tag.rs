//! `AnySlotTag` packed into [`HeapHeader::flags`] bits 8..11.
//!
//! v0.7 Phase-3 Step 5 lays the layout: the 4-bit `AnySlotTag`
//! discriminant lives in `flags[8..11]`, disjoint from the existing
//! flag bits (1-5) and the cycle-collector color bits (3-4 — only
//! valid on container types, see the disjoint-use rationale on the
//! `Color` enum). Step 5d then drops the dedicated `AnyBox::tag: i64`
//! field, shrinking `AnyBox` from 24 → 16 B (the 16 B fast-path size
//! class in `torajs-mmalloc`).
//!
//! This module owns the constants, the `impl HeapHeader { ... }`
//! accessors, and (in tests, kept in `lib.rs`) the round-trip
//! invariants. It is split out from `lib.rs` solely to keep that
//! file under the project-wide 500-prod-LOC hard limit per
//! `.claude/rules/common/file-size.md`.

use super::HeapHeader;

/// Bit position of the 4-bit `AnySlotTag` field in
/// [`HeapHeader::flags`]. Only meaningful on `Tag::AnyBox` headers.
pub const ANY_TAG_SHIFT: u16 = 8;
/// 4-bit mask covering bits 8..11 — the `AnySlotTag` slot.
pub const ANY_TAG_MASK: u16 = 0b1111 << ANY_TAG_SHIFT;

impl HeapHeader {
    /// Read the packed `AnySlotTag` discriminant from
    /// [`HeapHeader::flags`] bits 8..11.
    ///
    /// Only meaningful on `Tag::AnyBox` headers (Step 5+); on every
    /// other type the bits are zero. Returns the raw 4-bit value —
    /// `AnySlotTag` re-construction lives at the caller (so this
    /// helper stays panic-free / discriminant-agnostic).
    #[inline]
    pub fn any_tag_bits(&self) -> u16 {
        (self.flags & ANY_TAG_MASK) >> ANY_TAG_SHIFT
    }

    /// Write the 4-bit `AnySlotTag` discriminant into
    /// [`HeapHeader::flags`] bits 8..11. Preserves all other flag
    /// bits (FROZEN / BUFFERED / COLOR / etc).
    ///
    /// Callers should pass `tag as u16` where `tag: AnySlotTag` —
    /// the value's domain (0..=5) fits inside the 4-bit field; values
    /// > 0xF are silently masked.
    #[inline]
    pub fn set_any_tag(&mut self, tag_bits: u16) {
        self.flags = (self.flags & !ANY_TAG_MASK) | ((tag_bits << ANY_TAG_SHIFT) & ANY_TAG_MASK);
    }
}
