//! The lazily filled DFA for programs whose table outgrows the eager
//! cap. It carries on from the capped build's pool and fills one
//! `(state, byte)` transition the first time a search needs it.
//!
//! An unfilled transition is a 0 in the row with its `known` bit
//! clear, so the executor's hot path reads the same words it reads in
//! an eager table and only its dead-slot branch learns to ask. The
//! entry rows are always filled whole: `first_viable_start` reads them
//! as complete rows. Lazy rows never carry the monotone bit (it needs
//! every row known), and `any_accept_before_byte` is set so every
//! zero-width-accept bit is consulted.
//!
//! Memory is bounded per program. Past the budget the cache is cleared
//! and refilled from the entry states; a search that keeps clearing
//! without getting through many bytes per state gives up and the Pike
//! VM takes over from the start position it had reached.
//!
//! Ownership: the table sits behind a raw pointer in its `Program`
//! (`Program::dfa_lazy`), allocated once by the RegExp constructor and
//! freed with the program. A search borrows it mutably for the
//! duration of one call; no reference to it outlives the call, and it
//! is only reached from the thread that owns the RegExp.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr::NonNull;

use crate::dfa::build::{PartialDfa, Starts, seed_starts};
use crate::dfa::expand::Expander;
use crate::dfa::intern::intern_state;
use crate::dfa::pool::Pool;
use crate::dfa::search::{DfaState, TX_ACCEPT_BIT, Table, mask_get, mask_set};
use crate::dfa::{DfaProgram, DfaStates};
use crate::program::Program;

/// Bytes a lazily filled table may hold before it is cleared, about
/// 7000 rows of 1104 bytes, fewer where the state keys are long. Giving
/// up is expensive here: the Pike VM fallback runs
/// once per start position, so a search that outgrows the cache turns
/// quadratic.
pub const LAZY_BUDGET_BYTES: usize = 8 << 20;

/// A search gives up once it has cleared this many times and the bytes
/// it walked since the last clear are fewer than
/// [`MIN_BYTES_PER_STATE`] per cached state.
const MIN_CLEARS: u32 = 3;
const MIN_BYTES_PER_STATE: usize = 10;

/// Rows kept free ahead of a fill: one fill interns its target and the
/// pending-class targets that target recurses into.
const ROW_SLACK: usize = 8;

pub struct LazyDfa {
    /// The rows at rest, as the table every reader takes. During a fill
    /// they are swapped into `pool.states`.
    dfa: DfaProgram,
    pool: Pool,
    /// Bit `b` of `known[s]` is set once `(s, b)` is filled.
    known: Vec<[u32; 8]>,
    exp: Expander,
    /// The state `exp` was last prepared for, `u32::MAX` for none.
    prepared: u32,
    split: bool,
    flags: u8,
    budget: usize,
    key_words: usize,
    /// A poisoned intern happened: the program is the Pike VM's.
    disabled: bool,
    clears: u32,
    walked: usize,
    walked_at_clear: usize,
    /// A fill answered `None` in this search.
    gave_up: bool,
    gave_up_at: Option<i64>,
}

impl LazyDfa {
    fn new(prog: &Program, flags: u8, part: PartialDfa, budget: usize) -> Option<Self> {
        let PartialDfa {
            mut pool,
            starts,
            split,
        } = part;
        let known = adopt_rows(&mut pool);
        let mut lazy = Self {
            dfa: table_view(Vec::new(), starts),
            pool,
            known,
            exp: Expander::new(prog),
            prepared: u32::MAX,
            split,
            flags,
            budget,
            key_words: 0,
            disabled: false,
            clears: 0,
            walked: 0,
            walked_at_clear: 0,
            gave_up: false,
            gave_up_at: None,
        };
        lazy.key_words = lazy.pool.keys.iter().map(Vec::len).sum();
        lazy.store_rows();
        lazy.fill_entries(prog).then_some(lazy)
    }

    /// Bytes held, by allocation rather than by use. Called with the
    /// rows at rest.
    fn mem(&self) -> usize {
        let rows = match &self.dfa.states {
            DfaStates::Owned(v) => v.capacity(),
            DfaStates::Static(s) => s.len(),
        };
        rows * core::mem::size_of::<DfaState>()
            + self.known.capacity() * core::mem::size_of::<[u32; 8]>()
            + self.key_bytes()
    }

    fn key_bytes(&self) -> usize {
        // a key and its item hold the same PCs
        self.key_words * 2 * core::mem::size_of::<u32>()
    }

    /// Grow the rows and the known bits toward the budget and no
    /// further: left to double, the vectors could hold close to twice
    /// the rows the budget pays for. Each row is priced with the key
    /// bytes an average state carries so far, since the keys of the rows
    /// still to come count against the same budget. Called with the rows
    /// in the pool.
    fn reserve_rows(&mut self) {
        let len = self.pool.states.len();
        let cap = self.pool.states.capacity();
        if len + ROW_SLACK <= cap {
            return;
        }
        let per = core::mem::size_of::<DfaState>()
            + core::mem::size_of::<[u32; 8]>()
            + self.key_bytes() / len.max(1);
        let fit = self.budget / per;
        let want = (cap * 2).max(64).min(fit).max(len + ROW_SLACK);
        self.pool.states.reserve_exact(want - len);
        self.known
            .reserve_exact(want.saturating_sub(self.known.len()));
    }

    fn rows(&mut self) -> &mut Vec<DfaState> {
        match &mut self.dfa.states {
            DfaStates::Owned(v) => v,
            DfaStates::Static(_) => unreachable!("a lazy table owns its rows"),
        }
    }

    /// Move the rows from the pool, where interning grows them, to the
    /// table the readers take.
    fn store_rows(&mut self) {
        let rows = core::mem::take(&mut self.pool.states);
        *self.rows() = rows;
    }

    fn take_rows(&mut self) {
        self.pool.states = core::mem::take(self.rows());
    }

    /// Fill every slot of the entry rows. False when that poisoned the
    /// program or alone overran the budget.
    fn fill_entries(&mut self, prog: &Program) -> bool {
        let starts = [
            self.dfa.start,
            self.dfa.start_mid_word,
            self.dfa.start_mid_nonword,
        ];
        for s in starts {
            for b in 0..=255u8 {
                if !mask_get(&self.known[s as usize], b) {
                    self.fill_one(prog, s, b);
                }
                if self.pool.poisoned {
                    self.disabled = true;
                    return false;
                }
            }
        }
        if self.mem() > self.budget {
            self.disabled = true;
        }
        !self.disabled
    }

    /// Fill `(s, b)`. The rows are in the pool for the duration.
    fn fill_one(&mut self, prog: &Program, s: u32, b: u8) {
        self.take_rows();
        self.reserve_rows();
        let before = self.pool.states.len();
        let item = core::mem::replace(
            &mut self.pool.items[s as usize],
            crate::dfa::pool::StateItem::empty(),
        );
        if self.prepared != s {
            self.exp.prepare(prog, &item, self.split);
            self.prepared = s;
        }
        let (next, accept_before) =
            self.exp
                .step(prog, &item, b, self.split, self.flags, &mut self.pool);
        self.pool.items[s as usize] = item;
        let after = self.pool.states.len();
        self.key_words += self.pool.keys[before..after]
            .iter()
            .map(Vec::len)
            .sum::<usize>();
        self.known.resize(after, [0; 8]);
        self.pool.work.clear();
        let accept_bit = if self.pool.states[next as usize].is_accept {
            TX_ACCEPT_BIT
        } else {
            0
        };
        let row = &mut self.pool.states[s as usize];
        row.transitions[b as usize] = next | accept_bit;
        if accept_before && !row.is_accept {
            mask_set(&mut row.accept_before_byte, b);
        }
        mask_set(&mut self.known[s as usize], b);
        self.store_rows();
    }

    /// Fill `(s, b)` for a search standing `cursor` bytes into its
    /// current probe. Answers the state's index, which a clear may
    /// have changed, or `None` when the search gives up.
    fn fill(&mut self, prog: &Program, s: u32, b: u8, cursor: usize) -> Option<u32> {
        let r = self.try_fill(prog, s, b, cursor);
        self.gave_up |= r.is_none();
        r
    }

    fn try_fill(&mut self, prog: &Program, s: u32, b: u8, cursor: usize) -> Option<u32> {
        let mut s = s;
        if self.mem() > self.budget {
            let walked = self.walked + cursor;
            let since = walked - self.walked_at_clear;
            if self.clears >= MIN_CLEARS && since < MIN_BYTES_PER_STATE * self.known.len() {
                return None;
            }
            s = self.clear(prog, s)?;
            self.clears += 1;
            self.walked_at_clear = walked;
        }
        self.fill_one(prog, s, b);
        if self.pool.poisoned {
            self.disabled = true;
            return None;
        }
        Some(s)
    }

    /// Drop every cached state, reseed and refill the entry states,
    /// then re-intern state `s`; answers its new index.
    fn clear(&mut self, prog: &Program, s: u32) -> Option<u32> {
        let item = self.pool.items[s as usize].clone();
        self.pool.reset();
        let starts = seed_starts(prog, self.split, self.flags, &mut self.pool);
        let s = intern_state(
            prog,
            item.ready,
            item.deferred,
            item.attr,
            self.split,
            self.flags,
            &mut self.pool,
        );
        self.pool.work.clear();
        // fresh vectors: a cleared one would keep its allocation, and
        // with it the reason for the clear
        self.known = alloc::vec![[0; 8]; self.pool.states.len()];
        self.key_words = self.pool.keys.iter().map(Vec::len).sum();
        self.prepared = u32::MAX;
        self.dfa = table_view(Vec::new(), starts);
        self.store_rows();
        self.fill_entries(prog).then_some(s)
    }
}

/// Take over the capped build's rows: a row whose state is no longer
/// queued is complete, so its transitions get the destination's
/// accept bit folded in (as `fold_accept_bits` does for an eager
/// table, less the monotone bit) and every slot is known. Queued
/// states keep their empty rows.
fn adopt_rows(pool: &mut Pool) -> Vec<[u32; 8]> {
    let n = pool.states.len();
    let mut known = alloc::vec![[u32::MAX; 8]; n];
    for &q in &pool.work {
        known[q as usize] = [0; 8];
    }
    pool.work.clear();
    let accept: Vec<bool> = pool.states.iter().map(|s| s.is_accept).collect();
    for (row, k) in pool.states.iter_mut().zip(&known) {
        if k[0] != 0 {
            for t in row.transitions.iter_mut() {
                if accept[*t as usize] {
                    *t |= TX_ACCEPT_BIT;
                }
            }
        }
    }
    known
}

fn table_view(states: Vec<DfaState>, starts: Starts) -> DfaProgram {
    DfaProgram {
        states: DfaStates::Owned(states),
        start: starts[0],
        start_mid: starts[2],
        start_mid_word: starts[1],
        start_mid_nonword: starts[2],
        all_starts_equal: starts[0] == starts[1] && starts[0] == starts[2],
        any_accept_before_byte: true,
        poisoned: false,
    }
}

/// The owning slot a `Program` keeps its lazy table in; null when the
/// program has none.
#[derive(Default)]
pub struct LazySlot(Option<NonNull<LazyDfa>>);

impl LazySlot {
    /// Carry a capped build on lazily. `None` when filling the entry
    /// states poisoned the program or overran the budget on their own;
    /// the caller then leaves the program to the Pike VM.
    pub fn from_partial(prog: &Program, flags: u8, part: PartialDfa) -> Option<Self> {
        Self::with_budget(prog, flags, part, LAZY_BUDGET_BYTES)
    }

    fn with_budget(prog: &Program, flags: u8, part: PartialDfa, budget: usize) -> Option<Self> {
        let lazy = LazyDfa::new(prog, flags, part, budget)?;
        Some(Self(Some(NonNull::from(Box::leak(Box::new(lazy))))))
    }

    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }

    /// The table a search walks, `None` without one.
    pub(crate) fn table(&self) -> Option<LazyTable> {
        self.0.map(LazyTable)
    }
}

impl Drop for LazySlot {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            // SAFETY: allocated by `with_budget` through `Box::leak`,
            // owned by this slot alone, and no search is in flight
            // while the program drops.
            drop(unsafe { Box::from_raw(p.as_ptr()) });
        }
    }
}

impl core::fmt::Debug for LazySlot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(if self.0.is_some() {
            "LazySlot(set)"
        } else {
            "LazySlot(none)"
        })
    }
}

/// One search's handle on a lazy table. Every method dereferences the
/// slot's pointer for the length of the call only.
#[derive(Clone, Copy)]
pub(crate) struct LazyTable(NonNull<LazyDfa>);

impl LazyTable {
    /// Reset the per-search counters. False when the table is disabled
    /// and the Pike VM serves the search.
    pub(crate) fn begin_search(self) -> bool {
        // SAFETY: see the module doc — one search at a time.
        let l = unsafe { &mut *self.0.as_ptr() };
        l.clears = 0;
        l.walked = 0;
        l.walked_at_clear = 0;
        l.gave_up = false;
        l.gave_up_at = None;
        !l.disabled
    }

    /// Where the search gave up, if it did: every start before it was
    /// tried and did not match.
    pub(crate) fn gave_up_at(self) -> Option<i64> {
        // SAFETY: as in `begin_search`.
        unsafe { (*self.0.as_ptr()).gave_up_at }
    }
}

impl Table for LazyTable {
    const LAZY: bool = true;

    #[inline]
    fn dfa(&self) -> &DfaProgram {
        // SAFETY: the reference lives no longer than the caller's use
        // of it, and the executor takes a fresh one after every fill.
        unsafe { &(*self.0.as_ptr()).dfa }
    }

    #[inline]
    fn known(&self, state: u32, byte: u8) -> bool {
        // SAFETY: as in `dfa`.
        unsafe { mask_get(&(&(*self.0.as_ptr()).known)[state as usize], byte) }
    }

    fn fill(&self, prog: &Program, state: u32, byte: u8, cursor: usize) -> Option<u32> {
        // SAFETY: the executor holds no reference into the table
        // across this call.
        unsafe { (*self.0.as_ptr()).fill(prog, state, byte, cursor) }
    }

    #[inline]
    fn note_walked(&self, bytes: usize) {
        // SAFETY: as in `fill`.
        unsafe { (*self.0.as_ptr()).walked += bytes }
    }

    #[inline]
    fn stop_at(&self, st: i64) -> bool {
        // SAFETY: as in `fill`. A fill that answered `None` left the
        // search to give up at the start it was probing.
        let l = unsafe { &mut *self.0.as_ptr() };
        if l.gave_up {
            l.gave_up_at = Some(st);
        }
        l.gave_up
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dfa::build::build_with_cap;
    use crate::dfa::{CappedDfa, build_dfa, build_dfa_capped};
    use crate::program::Inst;
    use crate::vm::search_from;

    const OVER_CAP: &str = "(a|b)*a(a|b){9}";

    fn compile(pat: &str, flags: u8, can_dfa: bool) -> Program {
        let mut p = crate::parser::Parser::new(pat.as_bytes(), flags);
        let mut root = p.parse().expect("test pattern must parse");
        let names = p.names.clone();
        crate::resolve::resolve_backrefs(&mut root, &names, p.n_captures, flags);
        let mut prog = Program::new();
        prog.can_dfa = can_dfa && crate::dfa::analyze(&root).is_eligible();
        crate::compiler::compile(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        prog.has_save = prog.any_save();
        prog
    }

    fn lazy_prog(pat: &str, flags: u8, cap: usize, budget: usize) -> Program {
        let mut prog = compile(pat, flags, true);
        let CappedDfa::Over(part) = build_with_cap(&prog, flags, cap) else {
            panic!("{pat} must outgrow a cap of {cap} states");
        };
        prog.dfa_lazy = LazySlot::with_budget(&prog, flags, part, budget).expect("entries fit");
        prog
    }

    fn lazy_of(prog: &Program) -> &LazyDfa {
        let p = prog.dfa_lazy.0.expect("lazy table");
        // SAFETY: no search is running; the slot owns the table
        unsafe { p.as_ref() }
    }

    fn haystacks(alphabet: &[u8], count: usize, max_len: u64) -> Vec<Vec<u8>> {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        (0..count)
            .map(|_| {
                let len = next() % (max_len + 1);
                (0..len)
                    .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                    .collect()
            })
            .collect()
    }

    fn hits(
        prog: &Program,
        flags: u8,
        hay: &[u8],
        dfa: Option<&DfaProgram>,
    ) -> Vec<Option<(i64, i64)>> {
        (0..=hay.len() as i64)
            .map(|from| search_from(prog, hay, from, flags, dfa).map(|m| (m.start, m.end)))
            .collect()
    }

    /// Lazy, eager and Pike agree on every start of every haystack.
    fn assert_agree(pat: &str, flags: u8, lazy: &Program, alphabet: &[u8], max_len: u64) {
        let eager_prog = compile(pat, flags, true);
        let eager = build_dfa(&eager_prog, flags);
        let pike = compile(pat, flags, false);
        for hay in haystacks(alphabet, 200, max_len) {
            let want = hits(&pike, flags, &hay, None);
            if !eager.poisoned {
                assert_eq!(
                    hits(&eager_prog, flags, &hay, Some(&eager)),
                    want,
                    "eager {hay:?}"
                );
            }
            assert_eq!(hits(lazy, flags, &hay, None), want, "lazy {hay:?}");
        }
    }

    #[test]
    fn past_the_cap_matches_eager_and_pike() {
        let prog = compile(OVER_CAP, 0, true);
        assert!(build_dfa(&prog, 0).states.len() > crate::dfa::DFA_EAGER_MAX_STATES);
        let CappedDfa::Over(part) = build_dfa_capped(&prog, 0) else {
            panic!("{OVER_CAP} must outgrow the eager cap");
        };
        let mut lazy = compile(OVER_CAP, 0, true);
        lazy.dfa_lazy = LazySlot::from_partial(&lazy, 0, part).expect("entries fit");
        assert_agree(OVER_CAP, 0, &lazy, b"abc", 40);
    }

    #[test]
    fn clears_keep_results() {
        let budget = 40 * (core::mem::size_of::<DfaState>() + 32);
        let lazy = lazy_prog(OVER_CAP, 0, 0, budget);
        assert_agree(OVER_CAP, 0, &lazy, b"ab", 40);
        // a long haystack over a 2^10-state table cannot stay in 40 states
        let hay: Vec<u8> = haystacks(b"ab", 8, 4000).concat();
        let _ = search_from(&lazy, &hay, 0, 0, None);
        let l = lazy_of(&lazy);
        assert!(l.clears > 0 || l.gave_up, "the search must have cleared");
        assert!(!l.disabled);
    }

    #[test]
    fn giving_up_hands_pike_the_probed_start() {
        let pat = "[abc]*a[ab]{9}c";
        let budget = 16 * (core::mem::size_of::<DfaState>() + 32);
        let lazy = lazy_prog(pat, 0, 0, budget);
        let pike = compile(pat, 0, false);
        // a candidate accept at 11, then a walk 16 rows cannot hold,
        // then the end of the longest match
        let mut hay = b"abbbbbbbbbc".to_vec();
        hay.extend(haystacks(b"ab", 12, 100).concat());
        hay.extend_from_slice(b"abbbbbbbbbc");
        let got = search_from(&lazy, &hay, 0, 0, None).map(|m| (m.start, m.end));
        assert_eq!(lazy_of(&lazy).gave_up_at, Some(0));
        assert_eq!(got, Some((0, hay.len() as i64)));
        let mut gave_up = 0;
        for from in 0..=hay.len() as i64 {
            let want = search_from(&pike, &hay, from, 0, None).map(|m| (m.start, m.end));
            let got = search_from(&lazy, &hay, from, 0, None).map(|m| (m.start, m.end));
            assert_eq!(got, want, "from {from}");
            gave_up += lazy_of(&lazy).gave_up_at.is_some() as usize;
        }
        assert!(gave_up > 0);
        assert!(!lazy_of(&lazy).disabled);
    }

    #[test]
    fn word_boundary_past_the_cap() {
        let pat = "\\b(a|b)*a(a|b){8}\\b";
        let lazy = lazy_prog(pat, 0, 1, LAZY_BUDGET_BYTES);
        assert_agree(pat, 0, &lazy, b"ab c", 30);
    }

    #[test]
    fn poison_past_the_cap_falls_to_pike() {
        let flags = crate::parser::RE_FLAG_U;
        let pat = "(a|b)*a(a|b){3}(\\p{L}|\\p{N})";
        let lazy = lazy_prog(pat, flags, 1, LAZY_BUDGET_BYTES);
        assert!(
            !lazy_of(&lazy).disabled,
            "the poison sits past the entry rows"
        );
        let hay = "aaaab\u{3b1}".as_bytes();
        let pike = compile(pat, flags, false);
        assert_eq!(hits(&lazy, flags, hay, None), hits(&pike, flags, hay, None));
        assert!(
            lazy_of(&lazy).disabled,
            "meeting the poison disables the table"
        );
        assert_agree(pat, flags, &lazy, "ab1\u{3b1}".as_bytes(), 24);
    }
}
