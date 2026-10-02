//! What a program's bytecode says about where a match can start —
//! derived once at compile time, read by every search.
//!
//! Two anchors: the literal first byte (`prefix_byte`, a memchr skip
//! ahead of the DFA / Pike VM) and the whole-program literal
//! (`literal`, which replaces the search outright). Both compile
//! entries — the runtime compiler and the AOT-baked one — derive them
//! here so the two can not drift.

use alloc::vec::Vec;

use crate::parser::RE_FLAG_I;
use crate::program::{Inst, Op, Program};
use crate::vm::MatchResult;

/// A `Char` the byte comparison can stand in for: its baked
/// ignore-case bit is clear (a byte compare can not match both cases).
fn is_plain_char(inst: &Inst) -> bool {
    Op::from_u8(inst.op) == Some(Op::Char) && inst.pad as u8 & RE_FLAG_I == 0
}

impl Program {
    /// Fill `prefix_byte` and `literal` from the emitted bytecode.
    ///
    /// `prefix_byte`: walk forward over zero-width bookkeeping ops
    /// (`Save` / `SaveReset`, `AnchorB` / `AnchorE`, `WBound` / `NWBound`) to the
    /// first byte-consuming op; a plain `Char` there is the anchor,
    /// any other shape leaves none. A literal past ASCII is a run
    /// of `Char` ops, one per UTF-8 byte, so its lead byte anchors too.
    ///
    /// `literal`: the program is `Char+ Match` and nothing else, every
    /// `Char` plain and ASCII.
    pub fn derive_search_anchors(&mut self) {
        for inst in &self.insts {
            match Op::from_u8(inst.op) {
                Some(
                    Op::Save | Op::SaveReset | Op::AnchorB | Op::AnchorE | Op::WBound | Op::NWBound,
                ) => continue,
                Some(Op::Char) => {
                    if is_plain_char(inst) {
                        self.prefix_byte = Some(inst.byte());
                    }
                    break;
                }
                _ => break,
            }
        }
        let Some((last, chars)) = self.insts.split_last() else {
            return;
        };
        let is_run = Op::from_u8(last.op) == Some(Op::Match)
            && !chars.is_empty()
            && chars.iter().all(|i| is_plain_char(i) && i.byte() < 0x80);
        if is_run {
            self.literal = Some(chars.iter().map(|i| i.byte()).collect::<Vec<u8>>());
        }
    }
}

/// Leftmost occurrence of the non-empty `lit` in `s` at or after
/// `from`. A literal program has no `Save`, so the hit carries no
/// saves row.
pub fn find_literal(s: &[u8], from: i64, lit: &[u8]) -> Option<MatchResult> {
    let mut st = usize::try_from(from).ok()?;
    let last_start = s.len().checked_sub(lit.len())?;
    let (first, rest) = (lit[0], &lit[1..]);
    while st <= last_start {
        st += s[st..=last_start].iter().position(|&c| c == first)?;
        // `rest.is_empty()` first: a one-byte literal would otherwise
        // pay a `memcmp` call for a zero-length compare on every hit.
        if rest.is_empty() || &s[st + 1..st + lit.len()] == rest {
            return Some(MatchResult::no_saves(st as i64, (st + lit.len()) as i64));
        }
        st += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_is_found_at_its_leftmost_start_at_or_after_from() {
        let m = find_literal(b"abcabc", 1, b"bc").unwrap();
        assert_eq!((m.start, m.end), (1, 3));
        let m = find_literal(b"abcabc", 2, b"bc").unwrap();
        assert_eq!((m.start, m.end), (4, 6));
    }

    #[test]
    fn literal_longer_than_the_rest_of_the_subject_misses() {
        assert!(find_literal(b"ab", 0, b"abc").is_none());
        assert!(find_literal(b"abc", 1, b"abc").is_none());
        assert!(find_literal(b"abc", 9, b"a").is_none());
    }

    #[test]
    fn first_byte_hit_with_a_tail_mismatch_keeps_scanning() {
        let m = find_literal(b"ababc", 0, b"abc").unwrap();
        assert_eq!((m.start, m.end), (2, 5));
    }
}
