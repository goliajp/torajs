//! The one-pass matcher's shape, and the reference walk over it.
//!
//! Positions and capture slots follow the Pike VM row: `out[0]` /
//! `out[1]` are the whole match, `out[2k]` / `out[2k + 1]` group `k`,
//! `-1` for a group that did not participate. A save carried by an
//! edge is written at the position the edge consumes; a save carried
//! by an accept is written at the accept position.
//!
//! One matcher walks one payload encoding: Latin-1 code units are
//! bytes, UTF-16 code units are 16-bit. Edges compare the unit, so a
//! Latin-1 range is the same range on either payload and only the
//! edges built for units above `0xFF` reach them. Under `u` / `v` the
//! UTF-16 walk takes a surrogate pair as one step, the way the Pike
//! VM takes the code point it transcodes to.

use alloc::vec;
use alloc::vec::Vec;

/// One code-unit range out of a state. Ranges of a state are disjoint
/// and sorted by `lo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub lo: u32,
    pub hi: u32,
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
    /// the walk reads 16-bit units; otherwise bytes
    pub wide: bool,
    /// a high surrogate followed by a low one is one step of the walk
    /// and one step of the start scan (`u` / `v` over UTF-16)
    pub pairs: bool,
}

/// Highest unit an edge of a `wide` walk can name.
pub const UNIT_MAX_WIDE: u32 = 0xFFFF;
/// Highest unit an edge of a narrow walk can name.
pub const UNIT_MAX_NARROW: u32 = 0xFF;

impl MatcherIr {
    pub fn unit_max(&self) -> u32 {
        if self.wide {
            UNIT_MAX_WIDE
        } else {
            UNIT_MAX_NARROW
        }
    }

    /// Leftmost-first search over the code units `s` from `start`:
    /// the end of the match, with the row in `out[..stride]`, or -1.
    pub fn exec<U: Copy + Into<u32>>(&self, s: &[U], start: usize, out: &mut [i64]) -> i64 {
        let stride = self.stride as usize;
        let mut work = vec![-1i64; stride];
        let mut st = start;
        while st <= s.len() {
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
                let u: u32 = s[pos].into();
                let Some(e) = node.edges.iter().find(|e| e.lo <= u && u <= e.hi) else {
                    break;
                };
                for &slot in &e.saves {
                    work[slot as usize] = pos as i64;
                }
                pos += self.step(s, pos);
                state = e.target as usize;
            }
            if last >= 0 {
                out[0] = st as i64;
                out[1] = last;
                return last;
            }
            st += self.step(s, st);
        }
        -1
    }

    /// Units one step from `pos` covers: two for a surrogate pair
    /// under `pairs`, one otherwise (`pos < s.len()`, or 1 at the end
    /// so a scan past it terminates).
    pub fn step<U: Copy + Into<u32>>(&self, s: &[U], pos: usize) -> usize {
        if self.pairs
            && pos + 1 < s.len()
            && is_high_surrogate(s[pos].into())
            && is_low_surrogate(s[pos + 1].into())
        {
            2
        } else {
            1
        }
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

pub fn is_high_surrogate(u: u32) -> bool {
    (0xD800..=0xDBFF).contains(&u)
}

pub fn is_low_surrogate(u: u32) -> bool {
    (0xDC00..=0xDFFF).contains(&u)
}
