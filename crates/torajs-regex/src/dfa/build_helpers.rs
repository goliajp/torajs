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

use crate::dfa::ctx::{PositionCtx, epsilon_closure_full};
use crate::program::{Op, Program};

/// chunk 8.6b — left-byte class carried as part of a DFA state's
/// identity. The just-consumed byte's word/non-word class is the only
/// thing `Op::WBound` / `Op::NWBound` need to see (besides the upcoming
/// right byte, supplied by the BFS step), so a 3-way attr lets us key
/// the state pool by `(PC set, attr)` without exploding into a per-
/// byte map. `TextStart` covers both "no preceding byte" and the
/// `RE_FLAG_M` line-start fold-in (left = `\n` is non-word so WBound
/// sees the same boundary as text-start when right is word-class).
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
/// for `at_word_boundary` (word vs non-word) and `AnchorB`-mflag
/// (`Some(b'\n')`); the wire folds the mflag line-start case into
/// `TextStart` so a representative `None`/`Some(b'a')`/`Some(b' ')`
/// is enough — mflag mid-pattern AnchorB re-fire (left = `\n`) is
/// handled separately when the BFS steps past `\n`, which surfaces in
/// the post-step set under whatever attr the actual byte produced.
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
/// chunk 8.6b — the at-end ctx now uses the state's left-byte attr
/// (instead of always `None`) and the compiled `mflag`. This lets
/// WBound resolve under `(left = attr, right = None / non-word)` so
/// patterns like `/\w$/` accept iff the live state's incoming byte
/// was word-class. mflag `Op::AnchorB` after `\n` doesn't trigger
/// here (the at-end check is about `$` / AnchorE, not `^`); but mflag
/// `AnchorE` *before* `\n` is folded in by the closure's right=None
/// path (right=None == non-word in `at_word_boundary`).
pub(super) fn pc_set_is_accept_at_end(
    prog: &Program,
    set: &BTreeSet<usize>,
    attr: LeftByteAttr,
    mflag: bool,
) -> bool {
    let seeds: Vec<usize> = set.iter().copied().collect();
    let at_end_ctx = PositionCtx {
        left_byte: attr_to_left_byte(attr),
        is_text_start: matches!(attr, LeftByteAttr::TextStart),
        is_text_end: true,
        mflag,
    };
    let closed = epsilon_closure_full(prog, &seeds, at_end_ctx, None);
    pc_set_is_accept(prog, &closed)
}

/// Round 3 Phase B sub-batch 4 attack #R-J v2 — shared `PositionCtx`
/// factory. Pulled out of `build_dfa`'s inline closure so
/// `intern_state` can also build at-cursor ctx values when computing
/// K-PROPERTY pending states' `yes_target` ε-closures.
pub(super) fn ctx_for(attr: LeftByteAttr, mflag: bool) -> PositionCtx {
    PositionCtx {
        left_byte: attr_to_left_byte(attr),
        is_text_start: matches!(attr, LeftByteAttr::TextStart),
        is_text_end: false,
        mflag,
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
