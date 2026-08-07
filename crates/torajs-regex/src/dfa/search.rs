//! DFA executor + `DfaProgram` data type.
//!
//! Houses the byte-walk that consumes a built [`DfaProgram`] and
//! returns the leftmost-longest match length, plus the four anchored
//! entry helpers the wire selects per cursor position. `DfaState`
//! struct + `PendingClass` re-export live in `super::state` (split out
//! when Round 3 Phase B sub-batch 6 attack #R-G pushed this file past
//! the 500-LOC HARD limit).
//!
//! Build-time setters (`build_dfa` / `intern_state` in
//! [`super::build`]) write through [`mask_set`] / public field
//! assignment; the executor only reads, so the executor never needs
//! a `&mut DfaState`.

pub use super::state::DfaState;

/// Round 5 attack #9 — accept-bit folding into the transition word.
///
/// `DfaState::transitions[b]` carries the destination state index in
/// the low 30 bits plus two flag bits describing the *destination*
/// state, folded in at build time by
/// [`super::build_helpers::fold_accept_bits`]:
///
/// - bit 31 ([`TX_ACCEPT_BIT`]) — destination's `is_accept`
/// - bit 30 ([`TX_MONOTONE_BIT`]) — destination's `monotone_accept`
///
/// The executor's per-byte hot loop then touches exactly one cache
/// line per step (the transition row) instead of two (row + the
/// destination state's flag bytes at offset 1024/1026, a different
/// 1072-byte-strided line). State 0 (dead) never accepts, so a packed
/// word of 0 still means "dead" and the `packed != 0` liveness test
/// is unchanged.
///
/// `monotone_accept` implies `is_accept`
/// ([`super::build_helpers::compute_monotone_accept`] condition 1),
/// so a self-loop slot inside a monotone run always reads exactly
/// `state | TX_ACCEPT_BIT | TX_MONOTONE_BIT` — [`run_monotone_accept`]
/// compares against that precomputed constant, keeping its inner loop
/// at one load + one compare per byte.
///
/// The `is_accept` / `monotone_accept` fields stay authoritative for
/// every cold read (start-state probe, pending-class fallback,
/// `fold_accept_bits` itself); the folded bits are a hot-path mirror.
/// BFS state counts are bounded far below 2^30 (`fold_accept_bits`
/// debug-asserts), so the two flag bits never collide with an index.
pub const TX_ACCEPT_BIT: u32 = 1 << 31;
/// See [`TX_ACCEPT_BIT`].
pub const TX_MONOTONE_BIT: u32 = 1 << 30;
/// Low-30-bit mask extracting the destination state index from a
/// packed transition word.
pub const TX_STATE_MASK: u32 = TX_MONOTONE_BIT - 1;

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
    /// RFC 20260711 chunk B — `true` when the BFS met a K-PROPERTY
    /// state shape the single-class pending mechanism cannot serve
    /// (content-distinct property classes in one ready set, e.g.
    /// `\p{L}|\p{N}`, or K-PROPERTY mixed with deferred bytes). A
    /// poisoned DFA would silently drop non-ASCII cps on those
    /// states, so every consumer (eager `dfa_runtime`, `resolve_dfa`
    /// local build, AOT bake) discards it and the cp-aware Pike VM
    /// serves the program. Never baked — `baked_dfa_view` hardcodes
    /// `false` because `try_bake_regex_dfa` refuses poisoned builds.
    pub poisoned: bool,
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

/// Round 3 Phase B sub-batch 6 attack #R-G — monotone-accept inner
/// tight loop. When `state.monotone_accept` is set, every
/// `transitions[b]` either self-loops (still an accepting state by
/// `compute_monotone_accept` invariant) or exits via a non-self
/// target (eventually dead). Skip the per-byte
/// `last_accept = Some(cursor)` store + the `is_accept` check inside
/// the run — the exit boundary writes `last_accept` once with the
/// same value. For `/\p{L}+/u` against `'  Hello42 word  '`, this
/// collapses the 5-byte letter-run's hot path from (load tx + cmp +
/// cond store last_accept) per byte to (load tx + cmp) per byte,
/// saving ~2-8 ns/iter.
///
/// Returns the cursor where the self-loop run ended — either the
/// first byte whose transition leaves `state` (caller's outer loop
/// re-dispatches it from the same cursor) or `hay.len()`. Both exits
/// are accepting positions (`monotone_accept` implies `is_accept`),
/// so the caller unconditionally records `last_accept` at the
/// returned cursor.
#[inline]
fn run_monotone_accept(dfa: &DfaProgram, hay: &[u8], state: u32, mut cursor: usize) -> usize {
    // Round 5 attack #9 — transitions are packed words. A self-loop
    // slot on a monotone-accept state always reads exactly
    // `state | TX_ACCEPT_BIT | TX_MONOTONE_BIT` (monotone implies
    // accept, and the destination IS this state), so one compare
    // against the precomputed constant keeps the inner loop at
    // load + cmp per byte — same shape as the pre-#9 `nxt != state`.
    let self_packed = state | TX_ACCEPT_BIT | TX_MONOTONE_BIT;
    while cursor < hay.len() {
        let b = hay[cursor];
        let nxt = dfa.states[state as usize].transitions[b as usize];
        if nxt != self_packed {
            break;
        }
        cursor += 1;
    }
    cursor
}

/// Round 3 Phase B sub-batch 4 attack #R-J v2 + v4 fallback — one
/// K-PROPERTY pending-class step: decode one UTF-8 cp at `cursor`
/// (1-4 bytes), call `prog.classes[class_idx].test_cp(cp)`, and
/// route to `yes_target` (matched, advance by utf8_len) or
/// `no_target` (not matched / invalid UTF-8, advance by 1 byte).
///
/// Returns `(next_state, new_cursor, at_end)`. `at_end == true` =
/// the sequence was truncated at the haystack end — the caller
/// breaks its loop with `alive = next_state != 0` (matching the
/// pre-split control flow where `cursor` is never read after that
/// break).
fn pending_class_step(
    prog: &crate::program::Program,
    pc: super::PendingClass,
    hay: &[u8],
    cursor: usize,
    byte: u8,
) -> (u32, usize, bool) {
    // Step 1: lead-byte → utf8_len classification.
    let utf8_len: usize = if byte < 0x80 {
        1
    } else {
        match byte {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => {
                // Invalid lead (0x80..0xC1 / 0xF5..0xFF): treat as
                // cp-miss, route to no_target, advance 1 byte so the
                // search loop can retry / exit.
                return (pc.no_target, cursor + 1, false);
            }
        }
    };
    // Step 2: truncated tail — abort sequence to no_target.
    if cursor + utf8_len > hay.len() {
        return (pc.no_target, cursor, true);
    }
    // Step 3: continuation-byte validity.
    for i in 1..utf8_len {
        if (hay[cursor + i] & 0xC0) != 0x80 {
            return (pc.no_target, cursor + 1, false);
        }
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
    let next = if matched { pc.yes_target } else { pc.no_target };
    (next, cursor + utf8_len, false)
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
        //
        // Round 5 attack #9 — the transition word carries the
        // destination's `is_accept` / `monotone_accept` flags in its
        // top two bits (see `TX_ACCEPT_BIT`), so this hot path reads
        // one cache line per byte instead of chasing the destination
        // state's flag bytes on a second line. Dead is still 0
        // (state 0 never accepts). The monotone probe nests inside
        // the accept branch — monotone implies accept — so the
        // non-accept step pays a single untaken branch.
        let packed = dfa.states[state as usize].transitions[byte as usize];
        if packed != 0 {
            state = packed & TX_STATE_MASK;
            cursor += 1;
            if packed & TX_ACCEPT_BIT != 0 {
                last_accept = Some(cursor);
                if packed & TX_MONOTONE_BIT != 0 {
                    cursor = run_monotone_accept(dfa, hay, state, cursor);
                    last_accept = Some(cursor);
                }
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
        let (next_state, new_cursor, at_end) = pending_class_step(prog, pc, hay, cursor, byte);
        state = next_state;
        cursor = new_cursor;
        if at_end {
            // Truncated tail at the haystack end. `cursor` is never
            // read after the break; the post-walk `is_accept_at_end`
            // check skips when `alive == false` (no_target == 0 =
            // dead state).
            alive = state != 0;
            break;
        }
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
    use crate::dfa::PendingClass;
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
        // Round 3 Phase B sub-batch 6 attack #R-G — monotone_accept
        // fills offset 1026, the first of the two bytes between
        // `is_accept_at_end` and `accept_before_byte`. Layout stays
        // 1072 bytes / align 4.
        assert_eq!(offset_of!(DfaState, monotone_accept), 1026);
        // The second one is `_pad`: declared rather than left to the
        // compiler because the AOT bake path reads this struct as raw
        // bytes, where an undeclared hole is uninitialised memory (it
        // made `tr build` emit two different binaries for one input).
        assert_eq!(offset_of!(DfaState, _pad), 1027);
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
                monotone_accept: false,
                _pad: 0,
                accept_before_byte: [0; 8],
                // Round 3 Phase B sub-batch 4 attack #R-J v2 — hand-
                // built `DfaState` literals must initialise the new
                // `pending_class` tail; `INERT` keeps the state on the
                // ordinary 256-way dispatch path.
                pending_class: PendingClass::INERT,
            },
            // start: byte 'a' moves to accept; everything else dies.
            // Round 5 attack #9 — hand-built tables must fold the
            // destination's accept flag into the transition word the
            // way `fold_accept_bits` does (state 2 is accepting, not
            // monotone).
            DfaState {
                transitions: {
                    let mut t = [0u32; 256];
                    t[b'a' as usize] = 2 | TX_ACCEPT_BIT;
                    t
                },
                is_accept: false,
                is_accept_at_end: false,
                monotone_accept: false,
                _pad: 0,
                accept_before_byte: [0; 8],
                pending_class: PendingClass::INERT,
            },
            // accept state.
            DfaState {
                transitions: [0; 256],
                is_accept: true,
                is_accept_at_end: false,
                monotone_accept: false,
                _pad: 0,
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
            poisoned: false,
        };
        assert_eq!(dfa_search(&dfa, &prog, b"a"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"abc"), Some(1));
        assert_eq!(dfa_search(&dfa, &prog, b"xyz"), None);
        // sanity: states[i] auto-deref-and-index path active.
        assert_eq!(dfa.states[1].transitions[b'a' as usize], 2 | TX_ACCEPT_BIT);
        assert!(dfa.states[2].is_accept);
    }
}
