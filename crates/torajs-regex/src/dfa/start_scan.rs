//! Where a match could begin — the scan that skips start positions
//! the entry state kills outright.
//!
//! Split out of [`super::search`] in rotation 472, along the line that
//! file's own module doc already draws: `search.rs` answers how long a
//! match starting at a position is, and this answers which positions
//! are worth asking about at all. The pending-class arm pushed the
//! parent past the 500-prod-LOC hard limit
//! (`rules/common/file-size.md`).

use super::program::DfaProgram;
use super::search::mask_get;
use super::state::DfaState;

/// First index at or after `from` where a match could begin, or
/// `hay.len()` when none can (the caller still probes that final
/// position — a pattern may accept zero-width at end of input).
///
/// Rotation 470 — the outer search loop used to answer this by
/// RUNNING the anchored DFA at every position and letting it die on
/// the first byte. Each doomed probe costs a call plus the search
/// prologue (~3-5 ns), and on a short haystack that was most of the
/// search: 66% of `/hello/i` against a 24-byte line (`examples/
/// match_micro`). Asking the entry state whether it has any live
/// transition on that byte answers the same question with one load.
///
/// A position is admitted when ANY entry state could get off the
/// ground there — `dfa_probe` picks among `start` / `start_mid_word`
/// / `start_mid_nonword` per the left-byte class, and taking the
/// union means this scan never has to reproduce that choice. Three
/// shapes admit unconditionally, all decided before the scan: an
/// entry that accepts empty (a zero-width match fits anywhere), one
/// carrying a pending multi-byte class (its lead byte legitimately
/// has no transition row), and — per byte — one whose
/// `accept_before_byte` bit is set (a zero-width accept at this
/// position).
///
/// This only skips positions the entry state kills on the FIRST byte.
/// A pattern like `[a-z]+@[a-z]+` starts happily on every letter and
/// dies deep into the word; those doomed probes need a different
/// instrument, so the scan is written to cost about one load when it
/// admits immediately.
#[inline]
pub fn first_viable_start(
    dfa: &DfaProgram,
    prog: &crate::program::Program,
    hay: &[u8],
    from: usize,
) -> usize {
    if from >= hay.len() {
        return hay.len();
    }
    if dfa.all_starts_equal {
        let st = &dfa.states[dfa.start as usize];
        if st.is_accept {
            return from;
        }
        let pc = st.pending_class;
        if pc.active != 0 {
            return pending_first_viable(dfa, prog, st, pc, hay, from);
        }
        let tx = &st.transitions;
        if dfa.any_accept_before_byte {
            let abb = &st.accept_before_byte;
            let mut i = from;
            while i < hay.len() {
                let b = hay[i];
                if tx[b as usize] != 0 || mask_get(abb, b) {
                    return i;
                }
                i += 1;
            }
            return hay.len();
        }
        let mut i = from;
        while i < hay.len() {
            if tx[hay[i] as usize] != 0 {
                return i;
            }
            i += 1;
        }
        return hay.len();
    }
    let entries = [dfa.start, dfa.start_mid_word, dfa.start_mid_nonword];
    for &e in &entries {
        let st = &dfa.states[e as usize];
        if st.is_accept || st.pending_class.active != 0 {
            return from;
        }
    }
    let mut i = from;
    while i < hay.len() {
        let b = hay[i];
        for &e in &entries {
            let st = &dfa.states[e as usize];
            if st.transitions[b as usize] != 0
                || (dfa.any_accept_before_byte && mask_get(&st.accept_before_byte, b))
            {
                return i;
            }
        }
        i += 1;
    }
    hay.len()
}

/// The pending-class twin of the row scan above. A K-PROPERTY entry
/// state has no 256-way row to ask — the class replaced it — so the
/// scan used to give up and let every position pay a full anchored
/// run. The class carries a 256-bit ASCII bitmap, which answers the
/// same question for any byte below 0x80.
///
/// Three shapes admit without deciding, and each is a real one:
///
/// - `no_target != 0`. A byte the class rejects still routes
///   somewhere live, so rejecting it does not make the position
///   hopeless. `/\p{L}+/u`'s entry dies on a non-letter, which is
///   exactly why its positions are skippable.
/// - a byte at or above 0x80. That is a UTF-8 lead byte whose code
///   point this scan will not decode; deciding it is the executor's
///   job.
/// - a live row slot or a zero-width accept at this byte. Either one
///   means something other than the pending class can start here.
///
/// `#[cold]` + out of line on purpose: it runs once per search, not
/// once per byte, and inlining it into the scan taxed the patterns
/// that have no pending class at all with its registers and its
/// share of the cache line — `/a.+c/s` measured 7% for a branch it
/// never takes.
#[cold]
#[inline(never)]
fn pending_first_viable(
    dfa: &DfaProgram,
    prog: &crate::program::Program,
    st: &DfaState,
    pc: super::PendingClass,
    hay: &[u8],
    from: usize,
) -> usize {
    if pc.no_target != 0 {
        return from;
    }
    let Some(class) = prog.classes.get(pc.class_idx as usize) else {
        return from;
    };
    let tx = &st.transitions;
    let abb = &st.accept_before_byte;
    let any_aab = dfa.any_accept_before_byte;
    let mut i = from;
    while i < hay.len() {
        let b = hay[i];
        if b >= 0x80
            || tx[b as usize] != 0
            || class.test_cp(b as i32)
            || (any_aab && mask_get(abb, b))
        {
            return i;
        }
        i += 1;
    }
    hay.len()
}
