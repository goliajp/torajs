//! The one-pass matcher's shape, and the reference walk over it.
//!
//! Positions and capture slots follow the Pike VM row: `out[0]` /
//! `out[1]` are the whole match, `out[2k]` / `out[2k + 1]` group `k`,
//! `-1` for a group that did not participate. A save carried by an
//! edge is written at the position the edge consumes; a save carried
//! by an accept is written at the accept position.

use alloc::vec;
use alloc::vec::Vec;

/// One byte range out of a state. Ranges of a state are disjoint and
/// sorted by `lo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub lo: u8,
    pub hi: u8,
    /// slots written (at the current position) when the edge is taken
    pub saves: Vec<u32>,
    pub target: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub edges: Vec<Edge>,
    /// `Some(saves)` when `Op::Match` is reachable: the highest-
    /// priority thread wins, so an accept with no edges answers at
    /// once, and an accept with edges is the fallback for a walk
    /// that dies later
    pub accept: Option<Vec<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatcherIr {
    /// state 0 is the start state; it is never an edge target
    pub states: Vec<State>,
    /// row width: highest save slot + 1, at least 2
    pub stride: u32,
}

impl MatcherIr {
    /// Leftmost-first search over Latin-1 code units from `start`:
    /// the end of the match, with the row in `out[..stride]`, or -1.
    pub fn exec(&self, s: &[u8], start: usize, out: &mut [i64]) -> i64 {
        let stride = self.stride as usize;
        let mut work = vec![-1i64; stride];
        for st in start..=s.len() {
            work.fill(-1);
            let mut state = 0usize;
            let mut pos = st;
            let mut last = -1i64;
            loop {
                let node = &self.states[state];
                if let Some(acc) = &node.accept {
                    out[2..stride].copy_from_slice(&work[2..stride]);
                    for &slot in acc {
                        out[slot as usize] = pos as i64;
                    }
                    last = pos as i64;
                    if node.edges.is_empty() {
                        break;
                    }
                }
                if pos == s.len() {
                    break;
                }
                let b = s[pos];
                let Some(e) = node.edges.iter().find(|e| e.lo <= b && b <= e.hi) else {
                    break;
                };
                for &slot in &e.saves {
                    work[slot as usize] = pos as i64;
                }
                pos += 1;
                state = e.target as usize;
            }
            if last >= 0 {
                out[0] = st as i64;
                out[1] = last;
                return last;
            }
        }
        -1
    }

    /// Slots a walk may still write after leaving an accept that has
    /// edges: everything reachable from such a state, its own accept
    /// included when a loop returns to it. A native walk that writes
    /// the row in place must hold these back at the accept and put
    /// them back when the walk dies.
    pub fn slots_written_past_accept(&self) -> Vec<u32> {
        let mut seen = vec![false; self.states.len()];
        let mut stack: Vec<usize> = Vec::new();
        let mut slots: Vec<u32> = Vec::new();
        for st in &self.states {
            if st.accept.is_some() && !st.edges.is_empty() {
                for e in &st.edges {
                    slots.extend_from_slice(&e.saves);
                    stack.push(e.target as usize);
                }
            }
        }
        while let Some(i) = stack.pop() {
            if seen[i] {
                continue;
            }
            seen[i] = true;
            let st = &self.states[i];
            if let Some(acc) = &st.accept {
                slots.extend_from_slice(acc);
            }
            for e in &st.edges {
                slots.extend_from_slice(&e.saves);
                stack.push(e.target as usize);
            }
        }
        slots.sort_unstable();
        slots.dedup();
        slots
    }
}
