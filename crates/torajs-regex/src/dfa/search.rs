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
///
/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-1 (2026-06-24) — `#[repr(C)]`
/// locks declared field order + C alignment so the AOT pipeline
/// (ssa_lower) can emit a byte-identical `[DfaState; N]` static into
/// `.rodata` and the `DfaStates::Static(&'static [...])` reader (Phase
/// B) deserialises it byte-for-byte. Layout on aarch64-apple-darwin:
///
/// | offset | size | field |
/// | ------ | ---- | ----- |
/// | 0      | 1024 | transitions[256] |
/// | 1024   | 1    | is_accept |
/// | 1025   | 1    | is_accept_at_end |
/// | 1026   | 2    | (padding to align 4) |
/// | 1028   | 32   | accept_before_byte[8] |
///
/// total = 1060 bytes, align 4. The unit test
/// `dfa_state_repr_c_layout_locked` enforces this layout so accidental
/// field-reorder / type-change breaks `cargo test` before reaching the
/// AOT byte emitter (where the same miscalculation would corrupt
/// `.rodata` silently).
#[repr(C)]
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

/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-1 (2026-06-24) — C-ABI
/// metadata struct emitted by the AOT pipeline (ssa_lower) alongside a
/// `[DfaState; N]` static into the user binary's `.rodata`. The
/// runtime constructor `__torajs_regex_compile_from_static_dfa`
/// (Phase C-2) reads one of these to build a `DfaProgram` whose
/// `states` is `DfaStates::Static(...)` over the baked slice, then
/// pre-fills `RegExp::dfa_cache` so the surface match path never
/// touches the (single-mutator UB-flaky) lazy build path for
/// AOT-eligible literal regexes — the central reason Phase C exists.
///
/// Layout on aarch64-apple-darwin:
///
/// | offset | size | field |
/// | ------ | ---- | ----- |
/// | 0      | 8    | states_ptr (chain-LC rebased at link) |
/// | 8      | 4    | states_len |
/// | 12     | 4    | start |
/// | 16     | 4    | start_mid |
/// | 20     | 4    | start_mid_word |
/// | 24     | 4    | start_mid_nonword |
/// | 28     | 4    | (tail padding to align 8) |
///
/// total = 32 bytes, align 8 (pointer-aligned, native word size). The
/// unit test `baked_dfa_meta_repr_c_layout_locked` enforces this
/// layout — accidental field-reorder or type-change is caught before
/// the AOT byte emitter rather than corrupting linker output.
#[repr(C)]
// Phase C-1 data type only; the runtime consumer
// `__torajs_regex_compile_from_static_dfa` lands in Phase C-2 and the
// ssa_lower-side AOT emitter in C-4/C-6. `#[expect(dead_code)]` would
// be more precise but flips unfulfilled under `cfg(test)` (the layout
// unit tests use `size_of` + `offset_of!` so the struct counts as
// constructed there). `#[allow(dead_code)]` is portable across both
// cfgs; the docstring + this comment record the "C-2 removes" plan.
#[allow(dead_code)]
pub struct BakedDfaMeta {
    /// Pointer to a `.rodata`-backed `[DfaState; states_len]` table.
    /// Rebased by the chain-LC dyld fixup chain at load time (same
    /// mechanism vtable + class_layouts ride on) so ASLR slide is
    /// honoured. Always non-null in well-formed input.
    pub states_ptr: *const DfaState,
    /// Number of `DfaState` entries at `states_ptr`. Stored as `u32`
    /// not `usize` since DFA state count is bounded by the BFS subset
    /// construction's state cap (well under 4 billion); locking the
    /// width keeps the C-ABI struct word-portable across host
    /// architectures should that ever matter.
    pub states_len: u32,
    /// Four anchored start state indices — exact mirror of
    /// `DfaProgram::{start, start_mid, start_mid_word, start_mid_nonword}`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    /// Phase C-1 layout lock — `DfaState` is the byte-for-byte ground
    /// truth the AOT pipeline (ssa_lower) emits into `.rodata`. Any
    /// reorder / type-change here must be matched in the byte emitter
    /// or the `.rodata` slice will be misread at runtime — catching
    /// drift at `cargo test` time is cheaper than diagnosing
    /// SIGSEGV / wrong-match silent corruption.
    #[test]
    fn dfa_state_repr_c_layout_locked() {
        assert_eq!(offset_of!(DfaState, transitions), 0);
        assert_eq!(offset_of!(DfaState, is_accept), 1024);
        assert_eq!(offset_of!(DfaState, is_accept_at_end), 1025);
        assert_eq!(offset_of!(DfaState, accept_before_byte), 1028);
        assert_eq!(size_of::<DfaState>(), 1060);
        assert_eq!(align_of::<DfaState>(), 4);
    }

    /// Phase C-1 layout lock — `BakedDfaMeta` mirrors the
    /// 32-byte AOT metadata struct ssa_lower emits per literal regex.
    /// See doc comment on the struct for the offset table.
    #[test]
    fn baked_dfa_meta_repr_c_layout_locked() {
        assert_eq!(offset_of!(BakedDfaMeta, states_ptr), 0);
        assert_eq!(offset_of!(BakedDfaMeta, states_len), 8);
        assert_eq!(offset_of!(BakedDfaMeta, start), 12);
        assert_eq!(offset_of!(BakedDfaMeta, start_mid), 16);
        assert_eq!(offset_of!(BakedDfaMeta, start_mid_word), 20);
        assert_eq!(offset_of!(BakedDfaMeta, start_mid_nonword), 24);
        assert_eq!(size_of::<BakedDfaMeta>(), 32);
        assert_eq!(align_of::<BakedDfaMeta>(), 8);
    }

    /// Phase B validation — a hand-built `DfaProgram` whose `states`
    /// uses `DfaStates::Static(&'static [...])` (the Phase C
    /// production shape) drives `dfa_search` correctly. Mirrors the
    /// trivial accept-on-byte-a fixture in `crate::dfa::tests::
    /// build_dfa_single_char_literal` so a regression in either
    /// `Owned` or `Static` deref path is locally caught.
    #[test]
    fn dfa_search_works_with_static_states() {
        // state 0 = dead; state 1 = start (transitions[b'a'] = 2);
        // state 2 = accept (self-loop dead).
        static STATES: [DfaState; 3] = [
            DfaState {
                transitions: [0; 256],
                is_accept: false,
                is_accept_at_end: false,
                accept_before_byte: [0; 8],
            },
            // start: byte 'a' moves to accept; everything else dies.
            DfaState {
                transitions: {
                    let mut t = [0u32; 256];
                    t[b'a' as usize] = 2;
                    t
                },
                is_accept: false,
                is_accept_at_end: false,
                accept_before_byte: [0; 8],
            },
            // accept state.
            DfaState {
                transitions: [0; 256],
                is_accept: true,
                is_accept_at_end: false,
                accept_before_byte: [0; 8],
            },
        ];
        let dfa = DfaProgram {
            states: DfaStates::Static(&STATES),
            start: 1,
            start_mid: 1,
            start_mid_word: 1,
            start_mid_nonword: 1,
        };
        assert_eq!(dfa_search(&dfa, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, b"abc"), Some(1));
        assert_eq!(dfa_search(&dfa, b"xyz"), None);
        // sanity: states[i] auto-deref-and-index path active.
        assert_eq!(dfa.states[1].transitions[b'a' as usize], 2);
        assert!(dfa.states[2].is_accept);
    }
}
