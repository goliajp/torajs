//! Helpers shared by the DFA subset-construction BFS in
//! [`super::build`]. Lives in a sibling file to keep `build.rs`
//! under the 500-line HARD limit while letting the BFS core focus on
//! the interning + work-queue logic.
//!
//! Contents:
//! - [`LeftByteAttr`] enum (chunk 8.6b) + the two byte/attr converters
//!   that thread it into the position-aware ε-closure.
//! - [`pc_set_is_accept`] / [`pc_set_is_accept_at_end`] — per-state
//!   accept analysers consumed by `intern_state`.
//! - [`ctx_for`] — `PositionCtx` factory pulled out of the BFS so
//!   `intern_state`'s K-PROPERTY pending recursion can build at-cursor
//!   contexts too.
//! - [`prog_uses_word_boundary`] — scanner driving the `needs_attr_split`
//!   decision in `build_dfa`.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use crate::dfa::DfaProgram;
use crate::dfa::ctx::{PositionCtx, epsilon_closure_full};
use crate::dfa::search::DfaState;
use crate::program::{Op, Program};

/// chunk 8.6b — left-byte class carried as part of a DFA state's
/// identity. The just-consumed byte's word/non-word class is the only
/// thing `Op::WBound` / `Op::NWBound` need to see (besides the upcoming
/// right byte, supplied by the BFS step), so a 3-way attr lets us key
/// the state pool by `(PC set, attr)` without exploding into a per-
/// byte map. `TextStart` covers both "no preceding byte" and the
/// all-multiline-`^` line-start fold-in (left = `\n` is non-word so
/// WBound sees the same boundary as text-start when right is
/// word-class; mixed per-inst m-bits are gated off the DFA in
/// `regex/compile.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum LeftByteAttr {
    TextStart,
    Word,
    NonWord,
}

/// Classify a byte's left-class for [`LeftByteAttr`]. Word class =
/// ASCII `[A-Za-z0-9_]`, mirroring [`crate::dfa::ctx::PositionCtx::
/// at_word_boundary`]'s `is_word_byte`.
pub(super) fn attr_of_byte(b: u8) -> LeftByteAttr {
    if b.is_ascii_alphanumeric() || b == b'_' {
        LeftByteAttr::Word
    } else {
        LeftByteAttr::NonWord
    }
}

/// Pick a representative left byte for a [`LeftByteAttr`] so the
/// closure can read `ctx.left_byte` uniformly. The value only matters
/// for `at_word_boundary` (word vs non-word) and multiline `AnchorB`
/// (`Some(b'\n')`); the wire folds the line-start case into
/// `TextStart` so a representative `None`/`Some(b'a')`/`Some(b' ')`
/// is enough — mid-pattern multiline AnchorB re-fire (left = `\n`)
/// is handled separately when the BFS steps past `\n`, which surfaces
/// in the post-step set under whatever attr the actual byte produced.
pub(super) fn attr_to_left_byte(attr: LeftByteAttr) -> Option<u8> {
    match attr {
        LeftByteAttr::TextStart => None,
        LeftByteAttr::Word => Some(b'a'),
        LeftByteAttr::NonWord => Some(b' '),
    }
}

/// True iff `set` contains any [`Op::Match`] PC.
pub(super) fn pc_set_is_accept(prog: &Program, set: &BTreeSet<usize>) -> bool {
    set.iter()
        .any(|&pc| pc < prog.len() && matches!(Op::from_u8(prog.insts[pc].op), Some(Op::Match)))
}

/// chunk 8.6a — true iff re-closing `set` under an at-end ctx
/// (`is_text_end = true`) reaches an [`Op::Match`] PC. Used at build
/// time to populate `DfaState::is_accept_at_end`; executor consults
/// the flag after the byte walk so `Op::AnchorE` (`$`) can fire at
/// the haystack end.
///
/// chunk 8.6b — the at-end ctx uses the state's left-byte attr
/// (instead of always `None`). This lets WBound resolve under
/// `(left = attr, right = None / non-word)` so patterns like `/\w$/`
/// accept iff the live state's incoming byte was word-class.
/// Multiline `Op::AnchorB` after `\n` doesn't trigger here (the
/// at-end check is about `$` / AnchorE, not `^`; the closure reads
/// the per-inst m-bit and `attr_to_left_byte` never answers `\n`).
pub(super) fn pc_set_is_accept_at_end(
    prog: &Program,
    set: &BTreeSet<usize>,
    attr: LeftByteAttr,
) -> bool {
    let seeds: Vec<usize> = set.iter().copied().collect();
    let at_end_ctx = PositionCtx {
        left_byte: attr_to_left_byte(attr),
        is_text_start: matches!(attr, LeftByteAttr::TextStart),
        is_text_end: true,
    };
    let closed = epsilon_closure_full(prog, &seeds, at_end_ctx, None);
    pc_set_is_accept(prog, &closed)
}

/// Round 3 Phase B sub-batch 4 attack #R-J v2 — shared `PositionCtx`
/// factory. Pulled out of `build_dfa`'s inline closure so
/// `intern_state` can also build at-cursor ctx values when computing
/// K-PROPERTY pending states' `yes_target` ε-closures.
pub(super) fn ctx_for(attr: LeftByteAttr) -> PositionCtx {
    PositionCtx {
        left_byte: attr_to_left_byte(attr),
        is_text_start: matches!(attr, LeftByteAttr::TextStart),
        is_text_end: false,
    }
}

/// chunk 8.6b — true iff the program (or any sub-program) emits any
/// `Op::WBound` / `Op::NWBound`. Drives the state-pool key strategy
/// in `intern_state`: programs with no `\b` / `\B` collapse
/// LeftByteAttr down to `TextStart`, recovering the pre-8.6b state
/// count (the attr field is a no-op for them).
pub(super) fn prog_uses_word_boundary(prog: &Program) -> bool {
    fn scan(insts: &[crate::program::Inst]) -> bool {
        insts
            .iter()
            .any(|ins| matches!(Op::from_u8(ins.op), Some(Op::WBound | Op::NWBound)))
    }
    if scan(&prog.insts) {
        return true;
    }
    for sub in prog.sub_progs.iter() {
        if scan(&sub.insts) {
            return true;
        }
    }
    false
}

/// Round 3 Phase B sub-batch 6 attack #R-G — fill the
/// `monotone_accept` bit on every built state. Called once by
/// [`super::build::build_dfa`] after BFS completes and all
/// `transitions[b]` slots / `pending_class` triples are populated.
/// Pure read-modify-write — no NFA / Program access.
///
/// A state is monotone-accept iff:
///   1. `is_accept == true`, AND
///   2. every non-dead `transitions[b]` (b in 0..256) lands on a
///      state with `is_accept == true`, AND
///   3. when `pending_class.active != 0`, both `yes_target` and
///      `no_target` either equal 0 (dead) or land on an
///      `is_accept` state.
///
/// The condition is conservative (a transition to a non-accept
/// state stays `false` even if the byte is practically
/// unreachable from any start), keeping the analysis cheap and
/// obviously correct.
/// Round 5 attack #9 — fold each destination state's `is_accept` /
/// `monotone_accept` flags into the top two bits of every
/// `transitions[b]` word (see [`super::search::TX_ACCEPT_BIT`]).
/// Called once by [`super::build::finish_dfa`], strictly AFTER
/// [`compute_monotone_accept`] (which reads transitions as plain
/// indices). The `is_accept` / `monotone_accept` fields stay
/// authoritative for cold reads; the folded bits are the hot-path
/// mirror the executor consumes so its per-byte step touches one
/// cache line instead of two. State 0 (dead) has both flags false,
/// so dead slots stay exactly 0 and the executor's `packed != 0`
/// liveness test is unchanged.
pub(super) fn fold_accept_bits(states: &mut [super::search::DfaState]) {
    use super::search::{TX_ACCEPT_BIT, TX_MONOTONE_BIT, TX_STATE_MASK};
    // BFS subset construction is bounded far below 2^30 states; the
    // two flag bits must never collide with a real index.
    debug_assert!(states.len() as u64 <= TX_STATE_MASK as u64);
    // `compute_monotone_accept` condition 1 makes monotone imply
    // accept. The executor leans on it: a word with the accept bit
    // clear carries no flag bits at all, so it reads the destination
    // index straight off the word without masking.
    debug_assert!(states.iter().all(|s| s.is_accept || !s.monotone_accept));
    let flags: alloc::vec::Vec<u32> = states
        .iter()
        .map(|s| {
            ((s.is_accept as u32) * TX_ACCEPT_BIT) | ((s.monotone_accept as u32) * TX_MONOTONE_BIT)
        })
        .collect();
    for s in states.iter_mut() {
        for t in s.transitions.iter_mut() {
            *t |= flags[*t as usize];
        }
    }
}

pub(super) fn compute_monotone_accept(states: &mut [super::search::DfaState]) {
    // Snapshot `is_accept` per state so the per-state loop below can
    // probe targets without re-borrowing `states[]` mutably.
    let is_accept: alloc::vec::Vec<bool> = states.iter().map(|s| s.is_accept).collect();
    for i in 1..states.len() {
        if !is_accept[i] {
            continue;
        }
        let mut monotone = true;
        // Probe every transition target. 0 (dead) is treated as a
        // valid exit — the monotone run ends, last_accept already
        // points at the right cursor.
        for b in 0..256 {
            let target = states[i].transitions[b] as usize;
            if target == 0 {
                continue;
            }
            if !is_accept[target] {
                monotone = false;
                break;
            }
        }
        // K-PROPERTY pending fallback (Round 3 Path A v2) — when a
        // state has `pending_class.active != 0`, the executor may
        // route a UTF-8 cp through `yes_target` (cp matches) or
        // `no_target` (cp misses / invalid). Both must land on
        // accepting (or dead) states to preserve monotonicity.
        if monotone && states[i].pending_class.active != 0 {
            let yes = states[i].pending_class.yes_target as usize;
            let no = states[i].pending_class.no_target as usize;
            if yes != 0 && !is_accept[yes] {
                monotone = false;
            }
            if monotone && no != 0 && !is_accept[no] {
                monotone = false;
            }
        }
        states[i].monotone_accept = monotone;
    }
}

/// Derive the post-BFS whole-DFA flags and assemble the
/// [`DfaProgram`].
pub(super) fn finish_dfa(
    mut states: Vec<DfaState>,
    start: u32,
    start_mid_word: u32,
    start_mid_nonword: u32,
    poisoned: bool,
) -> DfaProgram {
    // Round 3 Phase B attack #R-A2 — derive `all_starts_equal` from the
    // four BFS-interned start indices. `needs_attr_split == false` (no
    // `^` / `\b` / `\B` / multiline-`^` in pattern) already collapses
    // all four to the same state; the runtime wire reads this flag and
    // short-circuits the `at_line_start` / `prev_is_word` selection.
    // `start_mid_nonword` doubles as the returned `start_mid` field,
    // so checking equality against the two unique mid-entries is
    // sufficient (transitive: a == b && a == c implies b == c).
    let all_starts_equal = start == start_mid_word && start == start_mid_nonword;
    // Round 3 Phase B attack #R-E — derive `any_accept_before_byte` by
    // OR-ing every state's 256-bit mask. Set iff at least one bit is
    // non-zero. Cost: O(states × 8 u32 ORs) at build time (cold path,
    // amortised); the runtime hot loop reads a single bool. For
    // `/\p{L}+/u` (no `\b`) this short-circuits the per-byte mask
    // load + branch (~35 ns/iter saved).
    let any_accept_before_byte = states
        .iter()
        .any(|s| s.accept_before_byte.iter().any(|w| *w != 0));
    // Round 3 Phase B sub-batch 6 attack #R-G — derive
    // `monotone_accept` per state. A state is monotone-accept iff:
    // (1) it accepts (`is_accept == true`); AND
    // (2) every non-dead `transitions[b]` (b in 0..256) lands on a
    //     state that also accepts; AND
    // (3) if `pending_class.active != 0`, both `yes_target` and
    //     `no_target` either equal 0 (dead) or land on an accepting
    //     state.
    // The executor's hot path consults this bit to skip the per-byte
    // `last_accept = Some(cursor)` write inside a self-loop run on
    // `\p{L}+/u`-class patterns. Cost: O(states × 256) build-time scan
    // (cold path, ~0.3 µs for the 4-state `\p{L}+/u` DFA); the runtime
    // save is ~2-8 ns/iter on letter-heavy haystacks. Body lives in
    // `super::build_helpers::compute_monotone_accept` to keep build.rs
    // under the 500 LOC HARD limit.
    compute_monotone_accept(&mut states);
    // Round 5 attack #9 — fold each destination's `is_accept` /
    // `monotone_accept` into the top two bits of every transition
    // word, AFTER `compute_monotone_accept` (which reads transitions
    // as plain indices). The executor's per-byte step then reads one
    // cache line instead of two. Applies identically to the AOT bake
    // path (`try_bake_regex_dfa` serialises this function's output
    // byte-for-byte), so baked `.rodata` tables and the runtime
    // executor always agree on the encoding.
    fold_accept_bits(&mut states);
    DfaProgram {
        // chunk 7.7 v2 step 12 C2 Phase B — wrap as DfaStates::Owned;
        // Phase C will emit DfaStates::Static(&'static [...]) from the
        // tr build pipeline.
        states: crate::dfa::DfaStates::Owned(states),
        start,
        start_mid: start_mid_nonword,
        start_mid_word,
        start_mid_nonword,
        all_starts_equal,
        any_accept_before_byte,
        poisoned,
    }
}
