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

pub use super::program::DfaProgram;
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
/// A slot that transitions back into its own state therefore reads a
/// word known before the run starts — `state | TX_ACCEPT_BIT |
/// TX_MONOTONE_BIT` on a monotone-accepting state (`monotone_accept`
/// implies `is_accept`, see
/// [`super::build_helpers::compute_monotone_accept`] condition 1),
/// plain `state` on one that does not accept. [`run_self_loop`]
/// compares against that constant, keeping its inner loop at one load
/// and one compare per byte.
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

/// Drive a built [`DfaProgram`] over `hay`, returning the longest
/// prefix `hay[..n]` accepted by the anchored DFA — or `None` when no
/// prefix accepts. The executor never backtracks (the BFS already
/// folded leftmost-longest priority into the start state's transition
/// graph). Enters at [`DfaProgram::start`] (text-start ctx).
///
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
pub fn first_viable_start(dfa: &DfaProgram, hay: &[u8], from: usize) -> usize {
    if from >= hay.len() {
        return hay.len();
    }
    if dfa.all_starts_equal {
        let st = &dfa.states[dfa.start as usize];
        if st.is_accept || st.pending_class.active != 0 {
            return from;
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

/// Consume the run of bytes that all transition back into the state
/// whose row is `tx`. The row is fixed for the whole run, so the
/// address of each step's load no longer depends on the previous
/// step's loaded value — the loop-carried `load -> mask -> multiply
/// by the 1072-byte state stride -> load` recurrence that paces the
/// main byte walk collapses to a pipelined load + compare.
///
/// Every slot inside the run reads one precomputed word. Round 5
/// attack #9 packs the destination's `is_accept` / `monotone_accept`
/// into the transition word's top two bits, and the destination here
/// IS the source, so the whole word is known before the run starts:
/// `state` for a non-accepting state, `state | TX_ACCEPT_BIT |
/// TX_MONOTONE_BIT` for a monotone-accepting one.
///
/// Returns the cursor where the run ended — either the first byte
/// whose transition leaves the state (the caller's outer loop
/// re-dispatches it from that cursor) or `hay.len()`.
///
/// Two callers, and what each of them is allowed to skip inside the
/// run:
///
/// - Round 3 Phase B sub-batch 6 attack #R-G — a `monotone_accept`
///   state. Every position in the run accepts, so the per-byte
///   `last_accept = cursor` store is redundant with the one the
///   caller makes at the exit boundary. `/\p{L}+/u` against
///   `'  Hello42 word  '` rides this on its letter run.
///
/// - Rotation 472 — a state that does NOT accept. Nothing in the run
///   can move `last_accept` at all: the state has no accept bit, and
///   the caller has checked that it carries no zero-width accept
///   either. This is the shape `/a.+c/s` spends its search in — the
///   `.+` state self-loops on every byte but `c`, 16 of the 21 steps
///   over a 25-byte line, and `monotone_accept` never covered it
///   because that state is not an accepting one.
#[inline]
fn run_self_loop(hay: &[u8], tx: &[u32; 256], self_packed: u32, mut cursor: usize) -> usize {
    while cursor < hay.len() {
        if tx[hay[cursor] as usize] != self_packed {
            break;
        }
        cursor += 1;
    }
    cursor
}

/// True when no byte at this state carries a zero-width accept, so a
/// self-loop run may skip the per-byte `accept_before_byte` probe.
/// Only consulted when the program has any such mask at all — after
/// rotation 472 that means the pattern really does contain `\b` /
/// `\B` / multiline-`$`.
#[inline]
fn abb_is_empty(mask: &[u32; 8]) -> bool {
    mask.iter().all(|w| *w == 0)
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
    // Rotation 470 — two shapes the byte loop used to carry per step.
    // `dfa.states[state]` was written out three times, so LLVM had to
    // prove the three indexes equal and the bounds checks redundant;
    // the row is taken once per step now. And the accumulator was an
    // `Option<usize>`, which has no niche and so cost two word stores
    // on every accept; it is a plain index with `NO_ACCEPT` standing
    // for "none" — a haystack can never be `usize::MAX` bytes long, so
    // the sentinel is unreachable as a real position.
    const NO_ACCEPT: usize = usize::MAX;
    let states: &[DfaState] = &dfa.states;
    let mut state = start;
    let mut last_accept = NO_ACCEPT;
    if states[state as usize].is_accept {
        last_accept = 0;
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
        let st = &states[state as usize];
        // chunk 8.6b — zero-width accept before stepping byte at
        // `cursor`. Patterns like `/\bword\b/`: the trailing `\b`
        // resolves against the byte the cursor is about to consume
        // (right = byte) plus the just-consumed left byte; the
        // resulting Op::Match is zero-width so it never survives a
        // byte_step. The BFS precomputes `accept_before_byte` per state
        // per byte so the executor can record the accept here at
        // `cursor` (not `cursor + utf8_len`).
        if any_aab && mask_get(&st.accept_before_byte, byte) {
            last_accept = cursor;
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
        let packed = st.transitions[byte as usize];
        if packed != 0 {
            cursor += 1;
            if packed & TX_ACCEPT_BIT != 0 {
                let next = packed & TX_STATE_MASK;
                state = next;
                last_accept = cursor;
                if packed & TX_MONOTONE_BIT != 0 {
                    let tx = &states[next as usize].transitions;
                    cursor = run_self_loop(hay, tx, packed, cursor);
                    last_accept = cursor;
                }
                continue;
            }
            // No flag bits are set below here, so the word IS the
            // destination index — the accept-bit mask is only needed
            // on the branch above.
            if packed != state {
                state = packed;
                continue;
            }
            if !any_aab || abb_is_empty(&st.accept_before_byte) {
                // Rotation 472 — the same run, on a state that does
                // not accept. `packed` carries no flag bits here
                // (monotone implies accept), so `packed == state` is
                // exactly "this byte transitions back into the state
                // we are already in", and `st` is already the
                // destination's row. Nothing in the run can move
                // `last_accept`: the destination does not accept, and
                // the gate has established it carries no zero-width
                // accept either. `/a.+c/s` spends 16 of its 21 steps
                // here — `monotone_accept` never covered them because
                // the `.+` state is not an accepting one.
                cursor = run_self_loop(hay, &st.transitions, packed, cursor);
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
        let pc = st.pending_class;
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
        if states[state as usize].is_accept {
            last_accept = cursor;
        }
    }
    // chunk 8.6a — after consuming the haystack, if the live state's
    // PC set reaches `Op::Match` under an at-end ε-closure
    // (`Op::AnchorE` advances), record an accept at `hay.len()`. This
    // beats any earlier mid-byte accept since leftmost-longest wants
    // the longest match seen, and the at-end accept is by definition
    // the last `is_accept`-ish observation.
    if alive && states[state as usize].is_accept_at_end {
        last_accept = hay.len();
    }
    (last_accept != NO_ACCEPT).then_some(last_accept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dfa::{DfaStates, PendingClass};
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
