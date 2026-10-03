//! §12.3 LineTerminator over the UTF-8 (WTF-8) haystack the VM and
//! the DFA read: `\n`, `\r`, and U+2028 / U+2029 (`E2 80 A8` /
//! `E2 80 A9`). A `.` without `s` skips all four, and a multiline
//! `^` / `$` sits next to any of them.

use alloc::vec;
use alloc::vec::Vec;

use crate::parser::RE_FLAG_M;
use crate::program::{Op, Program};

/// A line terminator begins at byte `pos` of `s`.
#[inline]
pub fn at(s: &[u8], pos: usize) -> bool {
    match s.get(pos) {
        Some(b'\n' | b'\r') => true,
        Some(0xE2) => s.get(pos + 1) == Some(&0x80) && matches!(s.get(pos + 2), Some(0xA8 | 0xA9)),
        _ => false,
    }
}

/// A line terminator ends at byte `pos` of `s`.
#[inline]
pub fn before(s: &[u8], pos: usize) -> bool {
    match pos.checked_sub(1).map(|p| s[p]) {
        Some(b'\n' | b'\r') => true,
        Some(0xA8 | 0xA9) => pos >= 3 && s[pos - 3] == 0xE2 && s[pos - 2] == 0x80,
        _ => false,
    }
}

/// A multiline `^` the program reaches after consuming a character.
/// The DFA decides a `^` past its entry from the one byte before it,
/// which cannot tell U+2028 / U+2029 from another character ending in
/// `A8` / `A9`, so such a program is left to the VM. A `^` met before
/// anything is consumed is decided at entry, where the haystack is in
/// hand.
pub fn ml_line_start_after_consume(prog: &Program) -> bool {
    let n = prog.len();
    let mut seen = vec![false; n];
    let mut stack: Vec<usize> = prog
        .insts
        .iter()
        .enumerate()
        .filter(|(_, ins)| {
            matches!(
                Op::from_u8(ins.op),
                Some(Op::Char | Op::AnyChar | Op::Class)
            )
        })
        .map(|(pc, _)| pc + 1)
        .collect();
    while let Some(pc) = stack.pop() {
        if pc >= n || seen[pc] {
            continue;
        }
        seen[pc] = true;
        let ins = prog.insts[pc];
        match Op::from_u8(ins.op) {
            Some(Op::AnchorB) if ins.pad as u8 & RE_FLAG_M != 0 => return true,
            Some(Op::Jmp) => stack.push(ins.a as usize),
            Some(Op::Split) => {
                stack.push(ins.a as usize);
                stack.push(ins.b as usize);
            }
            Some(
                Op::AnchorB
                | Op::AnchorE
                | Op::WBound
                | Op::NWBound
                | Op::Save
                | Op::SaveReset
                | Op::IterStart
                | Op::IterCheck,
            ) => stack.push(pc + 1),
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_terminators_and_nothing_else() {
        let s = "a\nb\rc\u{2028}d\u{2029}e\u{e8}f\u{2027}".as_bytes();
        let starts: alloc::vec::Vec<usize> = (0..=s.len()).filter(|&p| at(s, p)).collect();
        assert_eq!(starts, [1, 3, 5, 9]);
        let ends: alloc::vec::Vec<usize> = (0..=s.len()).filter(|&p| before(s, p)).collect();
        assert_eq!(ends, [2, 4, 8, 12]);
    }

    #[test]
    fn a_line_start_past_a_character_is_found() {
        let reaches = |pat: &str, flags: u8| {
            let mut p = crate::parser::Parser::new(pat.as_bytes(), flags);
            let root = p.parse().unwrap();
            let mut prog = Program::new();
            crate::compiler::compile(&mut prog, &root, flags);
            prog.emit(crate::program::Inst::match_accept());
            ml_line_start_after_consume(&prog)
        };
        assert!(!reaches("^ab", RE_FLAG_M));
        assert!(!reaches("(?:^|x)b", RE_FLAG_M));
        assert!(reaches("a\\n^b", RE_FLAG_M));
        assert!(reaches("(?:^a)+", RE_FLAG_M));
        assert!(!reaches("a^b", 0));
    }
}
