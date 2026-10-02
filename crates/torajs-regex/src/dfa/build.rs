//! DFA builder — the eager subset construction that materialises the
//! [`super::search::DfaProgram`] consumed by
//! [`super::search::dfa_search`], and its capped form: a program whose
//! table would outgrow [`DFA_EAGER_MAX_STATES`] states gets none.
//! Interning lives in [`super::intern`], the per-byte step in
//! [`super::expand`].

use crate::dfa::DfaProgram;
use crate::dfa::build_helpers::{LeftByteAttr, ctx_for, finish_dfa, prog_uses_word_boundary};
use crate::dfa::expand::Expander;
use crate::dfa::intern::intern_state;
use crate::dfa::pool::{Pool, StateItem};
use crate::dfa::search::mask_set;
use crate::program::Program;

/// Most states an eagerly built table may have, the dead state
/// included. A table that would grow past it is never completed: the
/// runtime serves the program with the Pike VM and the AOT pipeline
/// bakes nothing.
pub const DFA_EAGER_MAX_STATES: usize = 256;

/// Most instructions a program may have and still be built eagerly.
/// Every state of a larger program pays a closure over that many PCs
/// for each of 256 bytes, and most of those states are never reached
/// by the haystacks it is run on.
pub const DFA_EAGER_MAX_INSTS: usize = 1024;

/// The three entry states: text-start, after a word byte, after a
/// non-word byte.
pub(super) type Starts = [u32; 3];

/// What [`build_dfa_capped`] produced.
pub enum CappedDfa {
    /// The complete table (possibly poisoned, which every consumer
    /// discards).
    Table(DfaProgram),
    /// The table outgrew the cap.
    Over,
}

/// Subset-construction NFA → DFA builder (chunks 8.5/8.6a/8.6b/8.7/
/// 8.8/9/10b), without a state cap. Seeds three start states
/// (`start` text-start; `start_mid_word` / `start_mid_nonword` per
/// the just-consumed byte's class; `start_mid` = `start_mid_nonword`
/// for back-compat). `flags` threads through the byte step (u-flag
/// deferred buckets, chunk 10b); the i / m / s bits are read per
/// instruction from `Inst.pad` (regexp-modifiers). Pool keys are
/// `(ready, deferred[3], LeftByteAttr)` so `Op::WBound` /
/// `Op::NWBound` resolution can differ across cursor classes (chunk
/// 8.6b) and in-flight u-flag multi-byte sequences don't conflate with
/// ready PCs (chunk 10b). State 0 = dead.
pub fn build_dfa(prog: &Program, flags: u8) -> DfaProgram {
    match build(prog, flags, usize::MAX) {
        CappedDfa::Table(dfa) => dfa,
        CappedDfa::Over => unreachable!("an uncapped build always completes"),
    }
}

/// [`build_dfa`] under [`DFA_EAGER_MAX_STATES`] and
/// [`DFA_EAGER_MAX_INSTS`]. The RegExp constructor and the AOT bake
/// both decide through this one function, so a table is baked exactly
/// when the runtime would have built it eagerly.
pub fn build_dfa_capped(prog: &Program, flags: u8) -> CappedDfa {
    let cap = if prog.len() > DFA_EAGER_MAX_INSTS {
        0
    } else {
        DFA_EAGER_MAX_STATES
    };
    build(prog, flags, cap)
}

/// [`build_dfa_capped`] with an explicit state cap.
#[cfg(test)]
pub(super) fn build_with_cap(prog: &Program, flags: u8, cap: usize) -> CappedDfa {
    build(prog, flags, cap)
}

fn build(prog: &Program, flags: u8, cap: usize) -> CappedDfa {
    let mut pool = Pool::new(prog);
    pool.reset();
    let split = prog_uses_word_boundary(prog);
    let starts = seed_starts(prog, split, flags, &mut pool);
    // A poisoned table is discarded by every consumer, so nothing
    // past the first unserveable intern is worth building: the
    // strings-property patterns poison on their very first state.
    if pool.poisoned {
        return CappedDfa::Table(finish(pool, starts));
    }
    let mut exp = Expander::new(prog);
    while let Some(&i) = pool.work.last() {
        if pool.states.len() > cap {
            return CappedDfa::Over;
        }
        pool.work.pop();
        expand_row(prog, i, split, flags, &mut pool, &mut exp);
        if pool.poisoned {
            break;
        }
    }
    CappedDfa::Table(finish(pool, starts))
}

fn finish(pool: Pool, starts: Starts) -> DfaProgram {
    finish_dfa(pool.states, starts[0], starts[1], starts[2], pool.poisoned)
}

/// Seed the three entry states in their fixed order.
fn seed_starts(prog: &Program, split: bool, flags: u8, pool: &mut Pool) -> Starts {
    [
        seed_start(prog, LeftByteAttr::TextStart, split, flags, pool),
        seed_start(prog, LeftByteAttr::Word, split, flags, pool),
        seed_start(prog, LeftByteAttr::NonWord, split, flags, pool),
    ]
}

/// Seed one start state: ε-close PC 0 under `attr`'s cursor ctx
/// (chunk 8.6b — `closure_with_ctx`, WBound terminal, not
/// `closure_full(.., None)`; see the closure-API role split note at
/// the top of dfa/mod.rs) and intern. The three start entries
/// (text-start / mid-word / mid-nonword) differ only in attr.
fn seed_start(prog: &Program, attr: LeftByteAttr, split: bool, flags: u8, pool: &mut Pool) -> u32 {
    let initial = pool.closure_sorted(prog, &[0], ctx_for(attr));
    intern_state(prog, initial, Default::default(), attr, split, flags, pool)
}

/// Expand all 256 transitions of state `i`. A poisoned intern leaves
/// the row unwritten; the caller stops.
fn expand_row(prog: &Program, i: u32, split: bool, flags: u8, pool: &mut Pool, exp: &mut Expander) {
    let item = core::mem::replace(&mut pool.items[i as usize], StateItem::empty());
    exp.prepare(prog, &item, split);
    let mut transitions = [0u32; 256];
    let mut accept_before_byte = [0u32; 8];
    for byte in 0..=255u8 {
        let (next, accept_before) = exp.step(prog, &item, byte, split, flags, pool);
        transitions[byte as usize] = next;
        if accept_before {
            mask_set(&mut accept_before_byte, byte);
        }
        if pool.poisoned {
            pool.items[i as usize] = item;
            return;
        }
    }
    pool.items[i as usize] = item;
    let row = &mut pool.states[i as usize];
    row.transitions = transitions;
    // The executor reads this mask to answer "is there a zero-width
    // accept sitting at this cursor, before the byte is stepped". On a
    // state that already accepts the answer is yes for every byte and
    // `last_accept` is already this cursor, so the bits would only
    // re-store the value that is there — and writing them left
    // `any_accept_before_byte` true for every pattern with a reachable
    // accepting state. Only non-accepting states carry a mask.
    if !row.is_accept {
        row.accept_before_byte = accept_before_byte;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::Inst;

    fn compile(pat: &str) -> Program {
        let mut p = crate::parser::Parser::new(pat.as_bytes(), 0);
        let root = p.parse().expect("test pattern must parse");
        let mut prog = Program::new();
        crate::compiler::compile(&mut prog, &root, 0);
        prog.emit(Inst::match_accept());
        prog.can_dfa = true;
        prog
    }

    #[test]
    fn cap_boundary_is_the_state_count() {
        let prog = compile("[a-c]x[a-c]y|ab+c");
        let n = build_dfa(&prog, 0).states.len();
        assert!(matches!(build_with_cap(&prog, 0, n), CappedDfa::Table(_)));
        assert!(matches!(build_with_cap(&prog, 0, n - 1), CappedDfa::Over));
        let CappedDfa::Table(capped) = build_dfa_capped(&prog, 0) else {
            panic!("a small table stays eager");
        };
        assert_eq!(capped.states.len(), n);
    }

    #[test]
    fn past_the_cap_stays_unbuilt() {
        let prog = compile("(a|b)*a(a|b){9}");
        assert!(build_dfa(&prog, 0).states.len() > DFA_EAGER_MAX_STATES);
        assert!(matches!(build_dfa_capped(&prog, 0), CappedDfa::Over));
    }
}
