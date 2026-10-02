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
//! UTF-16 walk reads a surrogate pair as one step and compares its
//! code point; a surrogate outside a pair is a step of its own.
//!
//! An assertion on a state's ε path is a question about the position:
//! at the input's start or end, at a line's, after or before a word
//! unit. A state names the bits its closure reads (`probes`) and
//! carries one [`Variant`] per answer, so the walk reads the bits at
//! the position and takes the variant they select.

use alloc::vec;
use alloc::vec::Vec;

/// One range of units out of a state — code points when the walk
/// reads pairs. Ranges of a variant are disjoint and sorted by `lo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub lo: u32,
    pub hi: u32,
    /// slots written (at the current position) when the edge is taken
    pub saves: Vec<u32>,
    /// slots unset when the edge is taken (a quantified atom's groups
    /// at the top of an iteration); disjoint from `saves`
    pub clears: Vec<u32>,
    pub target: u32,
}

/// A state's body under one answer to its probes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub edges: Vec<Edge>,
    /// `Some(saves)` when `Op::Match` is reachable: the highest-
    /// priority thread wins, so an accept with no edges answers at
    /// once, and an accept with edges is the fallback for a walk
    /// that dies later
    pub accept: Option<Vec<u32>>,
    /// slots unset at the accept, beside `accept`'s writes; empty when
    /// there is no accept
    pub accept_clears: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// the context bits this state's closure reads (`INPUT_START` …)
    pub probes: u8,
    /// one variant per assignment of the probed bits, at the index
    /// [`pack`] gives that assignment; one variant when nothing is
    /// probed
    pub variants: Vec<Variant>,
}

/// The position is 0.
pub const INPUT_START: u8 = 1;
/// The position is the length.
pub const INPUT_END: u8 = 2;
/// The position is 0 or follows a LineTerminator (`^` under `m`).
pub const LINE_START: u8 = 4;
/// The position is the length or precedes a LineTerminator (`$`
/// under `m`).
pub const LINE_END: u8 = 8;
/// The unit before the position is a word character.
pub const WORD_LEFT: u8 = 16;
/// The unit at the position is a word character.
pub const WORD_RIGHT: u8 = 32;
/// `WORD_LEFT` with the `iu` word characters (§22.2.2.9.1: `ſ` and
/// Kelvin `K` canonicalize into the ASCII word set).
pub const WORD_LEFT_FOLD: u8 = 64;
/// `WORD_RIGHT` with the `iu` word characters.
pub const WORD_RIGHT_FOLD: u8 = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatcherIr {
    /// state 0 is the start state; it is never an edge target, a loop
    /// back to the program's start enters a state of its own
    pub states: Vec<State>,
    /// row width: highest save slot + 1, at least 2
    pub stride: u32,
    /// the walk reads 16-bit units; otherwise bytes
    pub wide: bool,
    /// a high surrogate followed by a low one is one step of the walk
    /// and of the start scan, named by its code point (`u` / `v`, wide)
    pub pairs: bool,
    /// the walk tries `start` only (`y`)
    pub anchored: bool,
}

/// Highest unit an edge of a `pairs` walk can name.
pub const UNIT_MAX_PAIRS: u32 = 0x10FFFF;
/// Highest unit an edge of a `wide` walk without `pairs` can name.
pub const UNIT_MAX_WIDE: u32 = 0xFFFF;
/// Highest unit an edge of a narrow walk can name.
pub const UNIT_MAX_NARROW: u32 = 0xFF;

/// The variant index of the probed bits `probes` under the context
/// `ctx`: bit `k` of the index is the `k`-th probed bit, lowest first.
pub fn pack(probes: u8, ctx: u8) -> usize {
    let mut index = 0;
    let mut k = 0;
    for bit in 0..8 {
        if probes & (1 << bit) != 0 {
            if ctx & (1 << bit) != 0 {
                index |= 1 << k;
            }
            k += 1;
        }
    }
    index
}

impl State {
    pub fn variant(&self, ctx: u8) -> &Variant {
        &self.variants[pack(self.probes, ctx)]
    }

    pub fn accepts_anywhere(&self) -> bool {
        self.variants.iter().any(|v| v.accept.is_some())
    }
}

impl MatcherIr {
    pub fn unit_max(&self) -> u32 {
        if self.pairs {
            UNIT_MAX_PAIRS
        } else if self.wide {
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
                let v = node.variant(context(s, pos));
                if let Some(acc) = &v.accept {
                    out[2..stride].copy_from_slice(&work[2..stride]);
                    for &slot in acc {
                        out[slot as usize] = pos as i64;
                    }
                    for &slot in &v.accept_clears {
                        out[slot as usize] = -1;
                    }
                    last = pos as i64;
                    if v.edges.is_empty() {
                        break;
                    }
                }
                if pos == s.len() {
                    break;
                }
                let step = self.step(s, pos);
                let u = if step == 2 {
                    code_point(s[pos].into(), s[pos + 1].into())
                } else {
                    s[pos].into()
                };
                let Some(e) = v.edges.iter().find(|e| e.lo <= u && u <= e.hi) else {
                    break;
                };
                for &slot in &e.saves {
                    work[slot as usize] = pos as i64;
                }
                for &slot in &e.clears {
                    work[slot as usize] = -1;
                }
                pos += step;
                state = e.target as usize;
            }
            if last >= 0 {
                out[0] = st as i64;
                out[1] = last;
                return last;
            }
            if self.anchored {
                break;
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
    /// edges: everything reachable from such a variant, its state's
    /// own accepts included when a loop returns to it. A native walk
    /// that writes the row in place must hold these back at the
    /// accept and put them back when the walk dies.
    pub fn slots_written_past_accept(&self) -> Vec<u32> {
        let mut seen = vec![false; self.states.len()];
        let mut stack: Vec<usize> = Vec::new();
        let mut slots: Vec<u32> = Vec::new();
        for st in &self.states {
            for v in &st.variants {
                if v.accept.is_some() && !v.edges.is_empty() {
                    for e in &v.edges {
                        slots.extend_from_slice(&e.saves);
                        slots.extend_from_slice(&e.clears);
                        stack.push(e.target as usize);
                    }
                }
            }
        }
        while let Some(i) = stack.pop() {
            if seen[i] {
                continue;
            }
            seen[i] = true;
            for v in &self.states[i].variants {
                if let Some(acc) = &v.accept {
                    slots.extend_from_slice(acc);
                    slots.extend_from_slice(&v.accept_clears);
                }
                for e in &v.edges {
                    slots.extend_from_slice(&e.saves);
                    slots.extend_from_slice(&e.clears);
                    stack.push(e.target as usize);
                }
            }
        }
        slots.sort_unstable();
        slots.dedup();
        slots
    }
}

/// Every context bit at `pos` of `s`.
pub fn context<U: Copy + Into<u32>>(s: &[U], pos: usize) -> u8 {
    let before = (pos > 0).then(|| s[pos - 1].into());
    let at = (pos < s.len()).then(|| s[pos].into());
    let mut ctx = 0;
    if pos == 0 {
        ctx |= INPUT_START;
    }
    if pos == s.len() {
        ctx |= INPUT_END;
    }
    if before.is_none_or(is_line_terminator) {
        ctx |= LINE_START;
    }
    if at.is_none_or(is_line_terminator) {
        ctx |= LINE_END;
    }
    if before.is_some_and(|u| is_word(u, false)) {
        ctx |= WORD_LEFT;
    }
    if at.is_some_and(|u| is_word(u, false)) {
        ctx |= WORD_RIGHT;
    }
    if before.is_some_and(|u| is_word(u, true)) {
        ctx |= WORD_LEFT_FOLD;
    }
    if at.is_some_and(|u| is_word(u, true)) {
        ctx |= WORD_RIGHT_FOLD;
    }
    ctx
}

/// §12.3 LineTerminator.
pub fn is_line_terminator(u: u32) -> bool {
    matches!(u, 0x0A | 0x0D | 0x2028 | 0x2029)
}

/// §22.2.2.9.1 WordCharacters: the ASCII word set, plus the two units
/// that canonicalize into it under `iu` when `fold`.
pub fn is_word(u: u32, fold: bool) -> bool {
    matches!(u, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x5F)
        || (fold && matches!(u, 0x17F | 0x212A))
}

/// The code point a surrogate pair spells.
pub fn code_point(high: u32, low: u32) -> u32 {
    0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
}

pub fn is_high_surrogate(u: u32) -> bool {
    (0xD800..=0xDBFF).contains(&u)
}

pub fn is_low_surrogate(u: u32) -> bool {
    (0xDC00..=0xDFFF).contains(&u)
}
