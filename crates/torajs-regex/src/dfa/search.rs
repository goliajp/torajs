//! DFA executor + `DfaState` / `DfaProgram` data types.
//!
//! Houses the byte-walk that consumes a built [`DfaProgram`] and
//! returns the leftmost-longest match length, plus the four anchored
//! entry helpers the wire selects per cursor position. The data
//! types live here too because the executor is the only public
//! reader of every field — keeping struct + reader co-located in one
//! file makes intent obvious.
//!
//! Build-time setters (`build_dfa` / `intern_state` in
//! [`super::build`]) write through [`mask_set`] / public field
//! assignment; the executor only reads, so the executor never needs
//! a `&mut DfaState`.

/// One state of a [`DfaProgram`].
///
/// `transitions[byte]` is the destination state index (0 = dead state).
/// `is_accept` is true iff the PC set this state represents contains an
/// [`crate::program::Op::Match`] PC (i.e. the NFA can accept at this
/// byte position).
/// `is_accept_at_end` (chunk 8.6a) is true iff the PC set, when
/// re-closed under an at-end ctx (`is_text_end = true`), reaches an
/// [`crate::program::Op::Match`] — so the executor can accept a match
/// that depends on `$` (`Op::AnchorE`) firing at the hay end.
///
/// The 256-way transition table is dense — every byte slot is filled at
/// build time, so the executor is a single
/// `state = states[state].transitions[byte]` step per input byte.
/// Memory cost is `256 * 4 = 1024` bytes per state plus 1 byte for the
/// flag (padded to 4); future sparse map can replace it when state
/// counts blow past a hot-cache budget. Capture writes are handled by
/// the wire's second-pass Pike VM, not in the DFA itself (chunk 9).
pub struct DfaState {
    /// `transitions[byte]` = destination state index. 0 means dead.
    pub transitions: [u32; 256],
    /// True iff the NFA PC set behind this state contains [`crate::program::Op::Match`].
    pub is_accept: bool,
    /// chunk 8.6a — true iff re-closing the PC set under an at-end ctx
    /// (`is_text_end = true`) reaches [`crate::program::Op::Match`].
    /// Set offline at build time; consumed by [`dfa_search_from`] after
    /// the byte walk to honour `Op::AnchorE` (`$`) at the haystack end.
    pub is_accept_at_end: bool,
    /// chunk 8.6b — 256-bit packed mask. Bit `b` is set iff re-closing
    /// the PC set at this state with `right_byte = Some(b)` reaches
    /// `Op::Match`. Lets the executor record a zero-width accept at
    /// cursor `i` *before* stepping byte `hay[i]` — required for
    /// patterns like `/\bword\b/` where the trailing `\b` resolves
    /// against the byte the cursor is about to consume, but the
    /// resulting `Op::Match` is zero-width (it never survives a
    /// `byte_step` that doesn't consume).
    pub accept_before_byte: [u32; 8],
}

impl Default for DfaState {
    fn default() -> Self {
        Self {
            transitions: [0u32; 256],
            is_accept: false,
            is_accept_at_end: false,
            accept_before_byte: [0u32; 8],
        }
    }
}

/// Read bit `byte` of a 256-bit packed mask.
#[inline]
pub(super) fn mask_get(mask: &[u32; 8], byte: u8) -> bool {
    (mask[(byte >> 5) as usize] >> (byte & 31)) & 1 != 0
}

/// Set bit `byte` of a 256-bit packed mask.
#[inline]
pub(super) fn mask_set(mask: &mut [u32; 8], byte: u8) {
    mask[(byte >> 5) as usize] |= 1u32 << (byte & 31);
}

/// A built DFA — dense transition table + four anchored start state
/// indices. `states[0]` = dead (empty PC set, self-loops, never
/// accepts).
///
/// Start-state selection (chunk 8.6b):
/// - `start` — cursor at text-start (offset 0) or — under `RE_FLAG_M`
///   — immediately after `\n`. `is_text_start = true` so `^`
///   advances; left attr folds in as `TextStart` (no preceding byte
///   for WBound — `at_word_boundary` treats `None` as non-word).
/// - `start_mid_word` — cursor at offset > 0 with the just-consumed
///   byte being a word-class byte (`[A-Za-z0-9_]`). `^` blocks; WBound
///   sees `left = Word`.
/// - `start_mid_nonword` — cursor at offset > 0 with the just-
///   consumed byte being non-word and not a line-start trigger. `^`
///   blocks; WBound sees `left = NonWord`.
/// - `start_mid` — back-compat alias, equals `start_mid_nonword` (the
///   no-WBound case where `left_byte = None` had been the BFS seed).
///
/// Patterns without `Op::AnchorB` / `Op::WBound` / `Op::NWBound` dedup
/// all four start states down. Caller must gate via `prog.can_dfa`
/// + [`super::prog_ops_dfa_safe`].
/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase B (2026-06-24) — prep for
/// AOT-precompile DFA. Wraps `DfaProgram::states` so the same struct
/// supports both:
/// - runtime `build_dfa` returns `DfaStates::Owned(Vec<DfaState>)`
/// - tr build pipeline emits `DfaStates::Static(&'static [DfaState])`
///   from .rodata-baked tables (Phase C)
///
/// Read sites stay `dfa.states[i]` via `Deref<Target = [DfaState]>` +
/// auto-deref-and-index; no per-call-site changes needed beyond the
/// `build_dfa` wrap site. `DfaState` does NOT need `Clone` (vs Cow's
/// `ToOwned` requirement), so the slice-ification has zero cost on
/// the cold path.
pub enum DfaStates {
    Owned(alloc::vec::Vec<DfaState>),
    /// Reserved for Phase C — currently no producer; the variant
    /// lives so `dfa.states.deref()` lowers identically for both
    /// shapes today and stays stable when Phase C lands.
    Static(&'static [DfaState]),
}

impl core::ops::Deref for DfaStates {
    type Target = [DfaState];
    #[inline]
    fn deref(&self) -> &[DfaState] {
        match self {
            Self::Owned(v) => v.as_slice(),
            Self::Static(s) => s,
        }
    }
}

pub struct DfaProgram {
    pub states: DfaStates,
    pub start: u32,
    pub start_mid: u32,
    pub start_mid_word: u32,
    pub start_mid_nonword: u32,
}

/// Drive a built [`DfaProgram`] over `hay`, returning the longest
/// prefix `hay[..n]` accepted by the anchored DFA — or `None` when no
/// prefix accepts. The executor never backtracks (the BFS already
/// folded leftmost-longest priority into the start state's transition
/// graph). Enters at [`DfaProgram::start`] (text-start ctx).
pub fn dfa_search(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start)
}

/// Like [`dfa_search`] but enters at `dfa.start_mid_nonword` — back-
/// compat alias retained for callers that only need the legacy mid-
/// pattern entry. New code should prefer [`dfa_search_mid_word`] /
/// [`dfa_search_mid_nonword`] explicitly per the cursor's left-byte
/// class (chunk 8.6b).
pub fn dfa_search_mid(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start_mid)
}

/// chunk 8.6b — mid-pattern entry when the just-consumed byte at
/// `cursor - 1` is a word-class byte (`[A-Za-z0-9_]`).
pub fn dfa_search_mid_word(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start_mid_word)
}

/// chunk 8.6b — mid-pattern entry when the just-consumed byte at
/// `cursor - 1` is non-word and not a line-start (`\n`) trigger.
pub fn dfa_search_mid_nonword(dfa: &DfaProgram, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, hay, dfa.start_mid_nonword)
}

fn dfa_search_from(dfa: &DfaProgram, hay: &[u8], start: u32) -> Option<usize> {
    let mut state = start;
    let mut last_accept: Option<usize> = None;
    if dfa.states[state as usize].is_accept {
        last_accept = Some(0);
    }
    let mut alive = true;
    for (i, &byte) in hay.iter().enumerate() {
        // chunk 8.6b — zero-width accept before stepping byte `i`.
        // Patterns like `/\bword\b/`: the trailing `\b` resolves
        // against the byte the cursor is about to consume (right =
        // byte) plus the just-consumed left byte; the resulting
        // Op::Match is zero-width so it never survives a byte_step.
        // The BFS precomputes `accept_before_byte` per state per
        // byte so the executor can record the accept here at cursor
        // `i` (not `i + 1`).
        if mask_get(&dfa.states[state as usize].accept_before_byte, byte) {
            last_accept = Some(i);
        }
        state = dfa.states[state as usize].transitions[byte as usize];
        if state == 0 {
            alive = false;
            break;
        }
        if dfa.states[state as usize].is_accept {
            last_accept = Some(i + 1);
        }
    }
    // chunk 8.6a — after consuming the haystack, if the live state's
    // PC set reaches `Op::Match` under an at-end ε-closure
    // (`Op::AnchorE` advances), record an accept at `hay.len()`. This
    // beats any earlier mid-byte accept since leftmost-longest wants
    // the longest match seen, and the at-end accept is by definition
    // the last `is_accept`-ish observation.
    if alive && dfa.states[state as usize].is_accept_at_end {
        last_accept = Some(hay.len());
    }
    last_accept
}
