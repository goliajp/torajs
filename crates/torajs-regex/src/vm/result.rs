//! `MatchResult` shape for [`search_from`](super::search_from) /
//! [`match_anchor`](super::match_anchor).
//!
//! Round 3 Phase B sub-batch 5 attack #R-A3 — saves storage is a
//! two-variant enum (`MatchSaves::None` for programs without
//! `Op::Save`, `MatchSaves::Some` for those with). The hot-path
//! `NoSaves` arm skips the sentinel-init and the move into the
//! result slot; callers that never inspect captures (replaceAll
//! without `$N` / split without capture groups / matchAll on a
//! no-group pattern / DFA-only `test()`) take the `None` branch and
//! read an empty slice via the [`MatchResult::saves`] accessor.
//!
//! Chunk 801 — the boxed row is stride-sized per program (was a
//! fixed `[i64; REGEX_SAVE_SLOTS]`): the capture-group cap became a
//! sanity bound, so no fixed-width buffer may scale with it. Readers
//! index via [`save_slot`] — slots past the row answer the `-1`
//! "not captured" sentinel exactly like the old fixed-width padding.

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Read save slot `i` from a stride-sized row: in-row slots answer
/// their value, past-the-row slots answer the `-1` sentinel (the
/// old fixed-width buffers padded with `-1`; dynamic rows keep that
/// contract without the padding).
#[inline]
pub fn save_slot(saves: &[i64], i: usize) -> i64 {
    saves.get(i).copied().unwrap_or(-1)
}

/// Successful match outcome from [`search_from`](super::search_from)
/// / [`match_anchor`](super::match_anchor).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchResult {
    pub start: i64,
    pub end: i64,
    saves_repr: MatchSaves,
}

/// Storage backing [`MatchResult::saves`]. `None` is the
/// no-capture-group fast path (skips the row init); `Some` carries
/// the populated stride-sized slot row from a Pike VM second-pass.
///
/// Round 5 attack #5 — the row is boxed so the enum (and with it
/// `MatchResult`) stays 24 bytes: the no-save hot path used to
/// memcpy a 536-byte enum on every `Option<MatchResult>` move up
/// the call chain (profile: 4-6 ns/iter). The with-saves path
/// trades those repeated moves for one heap alloc per hit — that
/// path already pays a second-pass extraction, so the alloc is in
/// the noise there.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MatchSaves {
    None,
    Some(Box<[i64]>),
}

impl MatchResult {
    /// Hot-path constructor for a program with `prog.has_save == false`
    /// — skips the per-iter saves-row init that the inline-saves
    /// shape paid on every DFA hit.
    #[inline]
    pub fn no_saves(start: i64, end: i64) -> Self {
        Self {
            start,
            end,
            saves_repr: MatchSaves::None,
        }
    }

    /// Constructor for a program with `Op::Save` ops; carries the
    /// stride-sized slot row populated by the Pike VM second pass.
    /// (`vec![-1; stride]` rows have `len == capacity`, so the
    /// `into_boxed_slice` here never reallocates.)
    #[inline]
    pub fn with_saves(start: i64, end: i64, saves: Vec<i64>) -> Self {
        Self {
            start,
            end,
            saves_repr: MatchSaves::Some(saves.into_boxed_slice()),
        }
    }

    /// Read the saves row. Returns the live stride-sized row for
    /// `WithSaves`, or an empty slice for `NoSaves` — consumers
    /// index through [`save_slot`], which answers `-1` for every
    /// slot of an empty row, matching the old all-`-1` sentinel.
    #[inline]
    pub fn saves(&self) -> &[i64] {
        match &self.saves_repr {
            MatchSaves::None => &[],
            MatchSaves::Some(s) => s,
        }
    }
}
