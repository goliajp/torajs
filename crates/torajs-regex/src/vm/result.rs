//! `MatchResult` shape for [`search_from`](super::search_from) /
//! [`match_anchor`](super::match_anchor).
//!
//! Round 3 Phase B sub-batch 5 attack #R-A3 — saves storage is a
//! two-variant enum (`MatchSaves::None` for programs without
//! `Op::Save`, `MatchSaves::Some` for those with). The hot-path
//! `NoSaves` arm skips the 512-byte (`[i64; REGEX_SAVE_SLOTS]`)
//! sentinel-init (`[-1i64; REGEX_SAVE_SLOTS]`) and the move into the
//! result slot; callers that never inspect captures (replaceAll
//! without `$N` / split without capture groups / matchAll on a
//! no-group pattern / DFA-only `test()`) take the `None` branch and
//! read [`EMPTY_SAVES`] via the [`MatchResult::saves`] accessor for
//! backwards-compat.

use crate::node::REGEX_SAVE_SLOTS;

/// Successful match outcome from [`search_from`](super::search_from)
/// / [`match_anchor`](super::match_anchor).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchResult {
    pub start: i64,
    pub end: i64,
    saves_repr: MatchSaves,
}

/// Storage backing [`MatchResult::saves`]. `None` is the
/// no-capture-group fast path (skips the 512-byte stack init); `Some`
/// carries the populated slot array from a Pike VM second-pass.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MatchSaves {
    None,
    Some([i64; REGEX_SAVE_SLOTS]),
}

/// Shared all-`-1` sentinel returned by [`MatchResult::saves`] when
/// the program emitted no `Op::Save` ops. Same value the caller would
/// have stored inline pre-#R-A3, so caller-side reads of
/// `m.saves()[2*i]` continue to see `-1` for every group.
pub const EMPTY_SAVES: [i64; REGEX_SAVE_SLOTS] = [-1i64; REGEX_SAVE_SLOTS];

impl MatchResult {
    /// Hot-path constructor for a program with `prog.has_save == false`
    /// — skips the per-iter `[-1i64; REGEX_SAVE_SLOTS]` init that the
    /// inline-saves shape paid on every DFA hit.
    #[inline]
    pub fn no_saves(start: i64, end: i64) -> Self {
        Self {
            start,
            end,
            saves_repr: MatchSaves::None,
        }
    }

    /// Constructor for a program with `Op::Save` ops; carries the slot
    /// array populated by the Pike VM second pass.
    #[inline]
    pub fn with_saves(start: i64, end: i64, saves: [i64; REGEX_SAVE_SLOTS]) -> Self {
        Self {
            start,
            end,
            saves_repr: MatchSaves::Some(saves),
        }
    }

    /// Read the saves array. Returns the live slot array for
    /// `WithSaves`, or the all-`-1` sentinel ([`EMPTY_SAVES`]) for
    /// `NoSaves`. The `&'static EMPTY_SAVES` ref keeps the type the
    /// same as the field-access shape (`&[i64; REGEX_SAVE_SLOTS]`) so
    /// downstream consumers (`replace_fn::build_capture_strs`,
    /// `attach_groups`, `expand_repl`, `append_inner`) stay unchanged.
    #[inline]
    pub fn saves(&self) -> &[i64; REGEX_SAVE_SLOTS] {
        match &self.saves_repr {
            MatchSaves::None => &EMPTY_SAVES,
            MatchSaves::Some(s) => s,
        }
    }
}
