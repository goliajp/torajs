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
/// B) deserialises it byte-for-byte.
///
/// Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E, 2026-06-24) —
/// appended a 12-byte [`PendingClass`] triple for K-PROPERTY cp-step
/// support, growing the layout to 1072 bytes. The new field is
/// [`PendingClass::INERT`] for every state that is NOT a K-PROPERTY
/// pending state, so the executor's hot path adds only one byte read +
/// one branch on `pending_class.active != 0`. Layout on
/// aarch64-apple-darwin:
///
/// | offset | size | field |
/// | ------ | ---- | ----- |
/// | 0      | 1024 | transitions[256] |
/// | 1024   | 1    | is_accept |
/// | 1025   | 1    | is_accept_at_end |
/// | 1026   | 2    | (padding to align 4) |
/// | 1028   | 32   | accept_before_byte[8] |
/// | 1060   | 12   | pending_class (Round 3 Path A v2) |
///
/// total = 1072 bytes, align 4. The unit test
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
    /// Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E) — fixed-size
    /// per-state K-PROPERTY cp-step handler triple. When
    /// `pending_class.active != 0` the executor short-circuits the
    /// 256-way `transitions[byte]` lookup, decodes 1-4 UTF-8 bytes as
    /// one cp, calls
    /// `prog.classes[pending_class.class_idx].test_cp(cp)`, then
    /// transitions to `yes_target` (matched, advances by utf8_len) or
    /// `no_target` (not matched / invalid UTF-8). Default
    /// [`PendingClass::INERT`] (all-zero) keeps non-K-PROPERTY states on
    /// the existing dense-transitions fast path.
    pub pending_class: PendingClass,
}

impl Default for DfaState {
    fn default() -> Self {
        Self {
            transitions: [0u32; 256],
            is_accept: false,
            is_accept_at_end: false,
            accept_before_byte: [0u32; 8],
            pending_class: PendingClass::INERT,
        }
    }
}

// Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E) — `PendingClass`
// + the build-side K-PROPERTY predicates live in
// `super::pending_class`; re-exported through `dfa/mod.rs` so external
// callers see the unchanged `crate::dfa::PendingClass` surface.
pub use super::pending_class::PendingClass;

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
    /// Round 3 Phase B attack #R-A2 — `true` iff
    /// `start == start_mid == start_mid_word == start_mid_nonword`.
    /// Patterns without `^` / `\b` / `\B` / multiline-`^` dedup the
    /// four start indices during BFS (`needs_attr_split == false`); the
    /// runtime `at_line_start` + `prev_is_word` selection branch is
    /// then wasted work. The wire (`vm::search_from_with_ws`) reads
    /// this flag and short-circuits to a single `dfa_search` when set.
    pub all_starts_equal: bool,
    /// Round 3 Phase B attack #R-E — `true` iff at least one
    /// `DfaState` in `states` has a non-zero `accept_before_byte`
    /// mask (i.e. the pattern contains `\b` / `\B` / multiline-`$` /
    /// other zero-width accepts). When `false`, the per-byte
    /// `mask_get` call in `dfa_search_from` is wasted work and the
    /// executor gates it off, saving ~35 ns/iter on no-`\b` patterns
    /// (every fixture without word-boundary or other zero-width
    /// accept site benefits).
    pub any_accept_before_byte: bool,
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
/// Layout on aarch64-apple-darwin (Round 3 Phase B sub-batch 1
/// extension — `any_accept_before_byte` baked into existing tail pad
/// at offset 28; `OUTER_META_SIZE` stays 32):
///
/// | offset | size | field |
/// | ------ | ---- | ----- |
/// | 0      | 8    | states_ptr (chain-LC rebased at link) |
/// | 8      | 4    | states_len |
/// | 12     | 4    | start |
/// | 16     | 4    | start_mid |
/// | 20     | 4    | start_mid_word |
/// | 24     | 4    | start_mid_nonword |
/// | 28     | 1    | any_accept_before_byte (attack #R-E) |
/// | 29     | 3    | (tail padding to align 8) |
///
/// total = 32 bytes, align 8 (pointer-aligned, native word size). The
/// unit test `baked_dfa_meta_repr_c_layout_locked` enforces this
/// layout — accidental field-reorder or type-change is caught before
/// the AOT byte emitter rather than corrupting linker output.
///
/// Note `all_starts_equal` (attack #R-A2) is NOT baked — the four
/// `start*` indices already live in this struct, so the runtime
/// `baked_dfa_view` constructor derives the flag locally for ~5 ns
/// once per call (amortised across all 100k iters of a hot loop).
#[repr(C)]
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
    /// Round 3 Phase B attack #R-E — host-pre-computed mirror of
    /// `DfaProgram::any_accept_before_byte`. Lives in the 4-byte tail
    /// pad of the original layout so the struct size stays 32 bytes
    /// (`OUTER_META_SIZE` unchanged) and the chain-LC rebase slot at
    /// offset 0 doesn't move. ssa_lower computes this from the
    /// built DFA's per-state masks before serialising the entry.
    pub any_accept_before_byte: bool,
}

/// Drive a built [`DfaProgram`] over `hay`, returning the longest
/// prefix `hay[..n]` accepted by the anchored DFA — or `None` when no
/// prefix accepts. The executor never backtracks (the BFS already
/// folded leftmost-longest priority into the start state's transition
/// graph). Enters at [`DfaProgram::start`] (text-start ctx).
///
/// Round 3 Phase B sub-batch 4 attack #R-J v2 — takes `prog: &Program`
/// so the K-PROPERTY pending-class handler can read
/// `prog.classes[pending_class.class_idx]` for the `test_cp(cp)` call.
/// Patterns without K-PROPERTY pending states never touch `prog`.
pub fn dfa_search(dfa: &DfaProgram, prog: &crate::program::Program, hay: &[u8]) -> Option<usize> {
    dfa_search_from(dfa, prog, hay, dfa.start)
}

/// Like [`dfa_search`] but enters at `dfa.start_mid_nonword` — back-
/// compat alias retained for callers that only need the legacy mid-
/// pattern entry. New code should prefer [`dfa_search_mid_word`] /
/// [`dfa_search_mid_nonword`] explicitly per the cursor's left-byte
/// class (chunk 8.6b).
pub fn dfa_search_mid(
    dfa: &DfaProgram,
    prog: &crate::program::Program,
    hay: &[u8],
) -> Option<usize> {
    dfa_search_from(dfa, prog, hay, dfa.start_mid)
}

/// chunk 8.6b — mid-pattern entry when the just-consumed byte at
/// `cursor - 1` is a word-class byte (`[A-Za-z0-9_]`).
pub fn dfa_search_mid_word(
    dfa: &DfaProgram,
    prog: &crate::program::Program,
    hay: &[u8],
) -> Option<usize> {
    dfa_search_from(dfa, prog, hay, dfa.start_mid_word)
}

/// chunk 8.6b — mid-pattern entry when the just-consumed byte at
/// `cursor - 1` is non-word and not a line-start (`\n`) trigger.
pub fn dfa_search_mid_nonword(
    dfa: &DfaProgram,
    prog: &crate::program::Program,
    hay: &[u8],
) -> Option<usize> {
    dfa_search_from(dfa, prog, hay, dfa.start_mid_nonword)
}

fn dfa_search_from(
    dfa: &DfaProgram,
    prog: &crate::program::Program,
    hay: &[u8],
    start: u32,
) -> Option<usize> {
    let mut state = start;
    let mut last_accept: Option<usize> = None;
    if dfa.states[state as usize].is_accept {
        last_accept = Some(0);
    }
    let mut alive = true;
    // Round 3 Phase B attack #R-E — hoist `any_accept_before_byte` into
    // the loop guard. When the build pass observed no state with a
    // non-zero `accept_before_byte` mask (the common case: any pattern
    // without `\b` / `\B` / multiline-`$` / other zero-width accept
    // sites), skip the per-byte `mask_get` load + branch entirely. For
    // 100k-iter `/\p{L}+/u`-style fixtures this saves ~35 ns/iter; the
    // single hoisted bool check costs ~1 ns per call.
    let any_aab = dfa.any_accept_before_byte;
    // Round 3 Phase B sub-batch 4 attack #R-J v2 (§2.5.E) — the K-
    // PROPERTY pending-class handler must advance the cursor by the
    // UTF-8 byte length of the decoded cp (1-4 bytes), so we replace
    // the `for (i, &byte) in hay.iter().enumerate()` form with a
    // `while cursor < hay.len()` loop that owns its cursor explicitly.
    // Sibling fixtures still advance by 1 byte per iter and pay only
    // one extra `pending_class.active` byte-read + branch on the hot
    // path (sub-1 ns/iter on M-series ARM).
    let mut cursor: usize = 0;
    while cursor < hay.len() {
        let byte = hay[cursor];
        // chunk 8.6b — zero-width accept before stepping byte at
        // `cursor`. Patterns like `/\bword\b/`: the trailing `\b`
        // resolves against the byte the cursor is about to consume
        // (right = byte) plus the just-consumed left byte; the
        // resulting Op::Match is zero-width so it never survives a
        // byte_step. The BFS precomputes `accept_before_byte` per state
        // per byte so the executor can record the accept here at
        // `cursor` (not `cursor + utf8_len`).
        if any_aab && mask_get(&dfa.states[state as usize].accept_before_byte, byte) {
            last_accept = Some(cursor);
        }
        // v4 (regex-024 regression fix) — try ordinary 256-way
        // transitions table dispatch FIRST. A non-zero next state
        // means byte_step already routed this byte (ASCII letters via
        // K-PROPERTY's ASCII bitmap, digits via a sibling non-K-
        // PROPERTY `Op::Class` in mixed-PC ready sets, etc.).
        let next = dfa.states[state as usize].transitions[byte as usize];
        if next != 0 {
            state = next;
            cursor += 1;
            if dfa.states[state as usize].is_accept {
                last_accept = Some(cursor);
            }
            continue;
        }
        // Round 3 Phase B sub-batch 4 attack #R-J v2 + v4 fallback —
        // K-PROPERTY pending-class handler. transitions[byte] was 0
        // (the ordinary byte_step couldn't route this byte). If the
        // state carries an active pending_class triple, decode one
        // UTF-8 cp at the cursor (1-4 bytes), call
        // `prog.classes[class_idx].test_cp(cp)`, and branch to
        // `yes_target` (matched, advance by utf8_len) or `no_target`
        // (not matched / invalid UTF-8 / truncated, advance by 1
        // byte). For "pure pending" states (singleton K-PROPERTY
        // ready set like `/\p{L}+/u`'s start), every byte routes
        // here; for "mixed pending" states (e.g. `(\p{L})(\d+)
        // (\p{L})/u`'s post-`\d+`-loop), only bytes the ordinary
        // byte_step couldn't claim arrive here — typically non-ASCII
        // UTF-8 lead bytes whose full cp is a K-PROPERTY member.
        let pc = dfa.states[state as usize].pending_class;
        if pc.active == 0 {
            // No pending fallback — the dead route is final.
            state = 0;
            alive = false;
            break;
        }
        // Step 1: lead-byte → utf8_len classification.
        let utf8_len: usize = if byte < 0x80 {
            1
        } else {
            match byte {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => {
                    // Invalid lead (0x80..0xC1 / 0xF5..0xFF): treat
                    // as cp-miss, route to no_target, advance 1
                    // byte so the search loop can retry / exit.
                    state = pc.no_target;
                    cursor += 1;
                    if state == 0 {
                        alive = false;
                        break;
                    }
                    if dfa.states[state as usize].is_accept {
                        last_accept = Some(cursor);
                    }
                    continue;
                }
            }
        };
        // Step 2: truncated tail — abort sequence to no_target,
        // exit the loop. `cursor` is never read after the break;
        // the post-walk `is_accept_at_end` check skips when
        // `alive == false` (no_target == 0 = dead state).
        if cursor + utf8_len > hay.len() {
            state = pc.no_target;
            alive = state != 0;
            break;
        }
        // Step 3: continuation-byte validity.
        let mut valid_cont = true;
        for i in 1..utf8_len {
            if (hay[cursor + i] & 0xC0) != 0x80 {
                valid_cont = false;
                break;
            }
        }
        if !valid_cont {
            state = pc.no_target;
            cursor += 1;
            if state == 0 {
                alive = false;
                break;
            }
            if dfa.states[state as usize].is_accept {
                last_accept = Some(cursor);
            }
            continue;
        }
        // Step 4: decode cp via the standard UTF-8 shift-OR pattern.
        let cp: i32 = match utf8_len {
            1 => byte as i32,
            2 => (((byte & 0x1F) as i32) << 6) | ((hay[cursor + 1] & 0x3F) as i32),
            3 => {
                (((byte & 0x0F) as i32) << 12)
                    | (((hay[cursor + 1] & 0x3F) as i32) << 6)
                    | ((hay[cursor + 2] & 0x3F) as i32)
            }
            4 => {
                (((byte & 0x07) as i32) << 18)
                    | (((hay[cursor + 1] & 0x3F) as i32) << 12)
                    | (((hay[cursor + 2] & 0x3F) as i32) << 6)
                    | ((hay[cursor + 3] & 0x3F) as i32)
            }
            _ => byte as i32, // unreachable — utf8_len ∈ {1,2,3,4}
        };
        // Step 5: cc.test_cp(cp) decides yes/no.
        let cls_idx = pc.class_idx as usize;
        let matched = if cls_idx < prog.classes.len() {
            prog.classes[cls_idx].test_cp(cp)
        } else {
            // Defensive: class_idx out of range = no match. Should
            // not happen in well-formed BFS output.
            false
        };
        state = if matched { pc.yes_target } else { pc.no_target };
        cursor += utf8_len;
        if state == 0 {
            alive = false;
            break;
        }
        if dfa.states[state as usize].is_accept {
            last_accept = Some(cursor);
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
    use crate::program::Program;
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
        // Round 3 Phase B sub-batch 4 attack #R-J v2 — pending_class
        // triple appended at offset 1060, growing the struct to 1072
        // bytes. align stays 4 (`u32` is widest field).
        assert_eq!(offset_of!(DfaState, pending_class), 1060);
        assert_eq!(size_of::<DfaState>(), 1072);
        assert_eq!(align_of::<DfaState>(), 4);
    }

    // Round 3 Phase B sub-batch 4 attack #R-J v2 — `PendingClass` has
    // its own layout-lock test in `dfa/pending_class.rs::tests::
    // pending_class_layout_locked`; the `DfaState` layout-lock above
    // covers its placement at offset 1060.

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
        // Round 3 Phase B attack #R-E — `any_accept_before_byte` sits
        // in the original 4-byte tail pad; struct stays 32 bytes /
        // align 8, so `OUTER_META_SIZE` in user_regex_baked_layout is
        // unchanged.
        assert_eq!(offset_of!(BakedDfaMeta, any_accept_before_byte), 28);
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
                // Round 3 Phase B sub-batch 4 attack #R-J v2 — hand-
                // built `DfaState` literals must initialise the new
                // `pending_class` tail; `INERT` keeps the state on the
                // ordinary 256-way dispatch path.
                pending_class: PendingClass::INERT,
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
                pending_class: PendingClass::INERT,
            },
            // accept state.
            DfaState {
                transitions: [0; 256],
                is_accept: true,
                is_accept_at_end: false,
                accept_before_byte: [0; 8],
                pending_class: PendingClass::INERT,
            },
        ];
        // Round 3 Phase B sub-batch 4 attack #R-J v2 — `dfa_search` now
        // takes `prog: &Program` to look up the K-PROPERTY class table
        // when a state's `pending_class.active != 0`. This fixture has
        // no K-PROPERTY pending state, so an empty `Program` is enough.
        let prog = Program::new();
        let dfa = DfaProgram {
            states: DfaStates::Static(&STATES),
            start: 1,
            start_mid: 1,
            start_mid_word: 1,
            start_mid_nonword: 1,
            all_starts_equal: true,
            any_accept_before_byte: false,
        };
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"abc"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"xyz"), None);
        // sanity: states[i] auto-deref-and-index path active.
        assert_eq!(dfa.states[1].transitions[b'a' as usize], 2);
        assert!(dfa.states[2].is_accept);
    }
}
