//! Bounded backtracker — capture extraction over a known match window.
//!
//! The DFA answers `[st, end)` without captures. This pass walks the
//! program depth-first in thread-priority order over that window with
//! one saves row mutated in place, an explicit job stack, and a
//! visited bitset over `(pc, pos)` that bounds the work at
//! `prog.len() * (window + 1)` steps (RE2 `BitState`, regex-automata
//! `BoundedBacktracker`). The first path to reach `Op::Match` at `end`
//! is the one the Pike VM's highest-priority thread takes, so the row
//! it leaves is the same row.

use super::char_eq;
use crate::parser::{RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, is_word_byte, unicode_mode};
use crate::program::{Inst, Op, Program};
use crate::utf8::{utf8_decode_cp, utf8_len_for};
use alloc::vec::Vec;

/// visited-bitset budget: 256 KiB
const VISITED_BITS_MAX: usize = 8 * 256 * 1024;

const OP_CHAR: u8 = Op::Char as u8;
const OP_ANY_CHAR: u8 = Op::AnyChar as u8;
const OP_CLASS: u8 = Op::Class as u8;
const OP_ANCHOR_B: u8 = Op::AnchorB as u8;
const OP_ANCHOR_E: u8 = Op::AnchorE as u8;
const OP_WBOUND: u8 = Op::WBound as u8;
const OP_NWBOUND: u8 = Op::NWBound as u8;
const OP_JMP: u8 = Op::Jmp as u8;
const OP_SPLIT: u8 = Op::Split as u8;
const OP_MATCH: u8 = Op::Match as u8;
const OP_SAVE: u8 = Op::Save as u8;

#[derive(Clone, Copy, Debug)]
enum Job {
    /// resume at `pc`, `off` bytes into the window
    Step { pc: u32, off: u32 },
    /// undo a capture write made on an abandoned path
    Restore { slot: u32, old: i64 },
}

/// Per-RegExp buffers, reused across matches.
#[derive(Debug)]
pub struct Backtrack {
    /// [`can_backtrack`] of the owning program, answered once
    pub eligible: bool,
    stack: Vec<Job>,
    visited: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// the row holds the captures of the match ending at `end`
    Match,
    /// no path reaches `Op::Match` at `end`; the row is back to its
    /// entry state
    NoMatch,
    /// a consume stepped over a multi-byte character. The Pike VM
    /// files such a thread under the next byte's step, so its result
    /// is not a function of `(pc, pos)` alone; the window goes to it.
    WideStep,
}

impl Backtrack {
    pub fn for_program(prog: &Program) -> Self {
        Self {
            eligible: can_backtrack(prog),
            stack: Vec::new(),
            visited: Vec::new(),
        }
    }
}

/// True iff every instruction is one this pass executes and every
/// control transfer stays inside the program. Backreferences make the
/// outcome at `(pc, pos)` depend on the captures, which the visited
/// bitset cannot key on; lookarounds run sub-programs. Both keep the
/// Pike VM.
pub fn can_backtrack(prog: &Program) -> bool {
    let n = prog.len();
    let in_range = |t: i32| t >= 0 && (t as usize) < n;
    let falls_through = |i: usize| i + 1 < n;
    prog.insts.iter().enumerate().all(|(i, ins)| match ins.op {
        OP_CHAR | OP_ANY_CHAR | OP_ANCHOR_B | OP_ANCHOR_E | OP_WBOUND | OP_NWBOUND => {
            falls_through(i)
        }
        OP_CLASS => falls_through(i) && ins.a >= 0 && (ins.a as usize) < prog.classes.len(),
        OP_SAVE => falls_through(i) && ins.a >= 0,
        OP_JMP => in_range(ins.a),
        OP_SPLIT => in_range(ins.a) && in_range(ins.b),
        OP_MATCH => true,
        _ => false,
    })
}

/// Does the window `[st, end]` fit the visited budget?
#[inline]
pub fn fits(prog: &Program, st: i64, end: i64) -> bool {
    prog.len() * ((end - st) as usize + 1) <= VISITED_BITS_MAX
}

/// Extract the captures of the match of `prog` over exactly
/// `[st, end)` into `row` (stride-wide, all `-1` on entry). Requires
/// `bt.eligible` and [`fits`].
pub fn captures_at(
    prog: &Program,
    s: &[u8],
    st: i64,
    end: i64,
    flags: u8,
    bt: &mut Backtrack,
    row: &mut [i64],
) -> Outcome {
    let width = (end - st) as usize + 1;
    let words = (prog.len() * width).div_ceil(64);
    if bt.visited.len() < words {
        bt.visited.resize(words, 0);
    }
    bt.visited[..words].fill(0);
    bt.stack.clear();
    bt.stack.push(Job::Step { pc: 0, off: 0 });
    let walker = Walker {
        prog,
        s,
        st,
        end,
        width,
        unicode: unicode_mode(flags),
    };
    while let Some(job) = bt.stack.pop() {
        match job {
            Job::Restore { slot, old } => row[slot as usize] = old,
            Job::Step { pc, off } => match walker.walk(pc as usize, st + off as i64, bt, row) {
                Outcome::NoMatch => {}
                done => return done,
            },
        }
    }
    Outcome::NoMatch
}

struct Walker<'a> {
    prog: &'a Program,
    s: &'a [u8],
    st: i64,
    end: i64,
    width: usize,
    unicode: bool,
}

impl Walker<'_> {
    /// Follow one path until it dies, matches, or takes a wide step.
    /// A split parks its second target on the stack, below any
    /// restore the first target's path pushes, so the row is back to
    /// the split-time state when the second target resumes.
    fn walk(&self, mut pc: usize, mut pos: i64, bt: &mut Backtrack, row: &mut [i64]) -> Outcome {
        loop {
            let bit = pc * self.width + (pos - self.st) as usize;
            let mask = 1u64 << (bit & 63);
            let word = &mut bt.visited[bit >> 6];
            if *word & mask != 0 {
                return Outcome::NoMatch;
            }
            *word |= mask;
            let ins = self.prog.insts[pc];
            match ins.op {
                OP_JMP => {
                    pc = ins.a as usize;
                    continue;
                }
                OP_SPLIT => {
                    bt.stack.push(Job::Step {
                        pc: ins.b as u32,
                        off: (pos - self.st) as u32,
                    });
                    pc = ins.a as usize;
                    continue;
                }
                OP_SAVE => {
                    let slot = ins.a as usize;
                    bt.stack.push(Job::Restore {
                        slot: slot as u32,
                        old: row[slot],
                    });
                    row[slot] = pos;
                }
                OP_MATCH => {
                    if pos == self.end {
                        return Outcome::Match;
                    }
                    return Outcome::NoMatch;
                }
                OP_CHAR | OP_ANY_CHAR | OP_CLASS => match self.consume(ins, pos) {
                    Some(1) => pos += 1,
                    Some(_) => return Outcome::WideStep,
                    None => return Outcome::NoMatch,
                },
                _ => {
                    if !self.assertion_holds(ins, pos) {
                        return Outcome::NoMatch;
                    }
                }
            }
            pc += 1;
        }
    }

    /// Bytes the consuming instruction advances at `pos`, `None` when
    /// it does not match. A path standing at `end` cannot consume and
    /// still finish at `end`.
    #[inline]
    fn consume(&self, ins: Inst, pos: i64) -> Option<i64> {
        if pos >= self.end {
            return None;
        }
        let slen = self.s.len() as i64;
        let b = self.s[pos as usize];
        let ul = utf8_len_for(b) as i64;
        let whole_char = ul >= 1 && pos + ul <= slen;
        match ins.op {
            OP_CHAR => char_eq(ins.byte(), b, ins.pad as u8).then_some(1),
            OP_ANY_CHAR => {
                if ins.pad as u8 & RE_FLAG_S == 0 && b == b'\n' {
                    return None;
                }
                Some(if whole_char { ul } else { 1 })
            }
            _ => {
                let cc = &self.prog.classes[ins.a as usize];
                let ci = ins.pad as u8 & RE_FLAG_I != 0;
                if !cc.byte_only && self.unicode && whole_char {
                    let (cp, dec_len) = utf8_decode_cp(&self.s[pos as usize..]);
                    let adv = if dec_len > 0 { dec_len as i64 } else { ul };
                    return cc.test_cp_fold(cp, ci).then_some(adv);
                }
                cc.test_fold(b, ci).then_some(1)
            }
        }
    }

    /// Zero-width assertions, read against the whole haystack: the
    /// window bounds the match, not the context around it.
    #[inline]
    fn assertion_holds(&self, ins: Inst, pos: i64) -> bool {
        let s = self.s;
        let slen = s.len() as i64;
        let ml = ins.pad as u8 & RE_FLAG_M != 0;
        match ins.op {
            OP_ANCHOR_B => pos == 0 || (ml && s[(pos - 1) as usize] == b'\n'),
            OP_ANCHOR_E => pos == slen || (ml && s[pos as usize] == b'\n'),
            op => {
                let left = pos > 0 && is_word_byte(s[(pos - 1) as usize]);
                let right = pos < slen && is_word_byte(s[pos as usize]);
                (left != right) == (op == OP_WBOUND)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::parser::{Parser, RE_FLAG_U};
    use crate::vm::match_at::vm_match_at;
    use crate::vm::{Workspace, search_from};
    use alloc::vec;
    use alloc::vec::Vec;

    fn build(pat: &str, flags: u8) -> Program {
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        prog.has_save = prog.any_save();
        prog.finalize_backref_caps();
        prog
    }

    fn pike(
        prog: &Program,
        s: &[u8],
        st: i64,
        end: i64,
        flags: u8,
        ws: &mut Workspace,
    ) -> Option<Vec<i64>> {
        let mut row = vec![-1i64; ws.arena.stride];
        (vm_match_at(prog, s, st, flags, ws, Some(&mut row), end) == end).then_some(row)
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Tally {
        matched: usize,
        missed: usize,
        wide: usize,
    }

    /// run both passes over every window of `hay` on one workspace
    /// and require equal rows
    fn compare_all_windows(pat: &str, flags: u8, hay: &[u8]) -> Tally {
        let prog = build(pat, flags);
        assert!(can_backtrack(&prog), "{pat}: not eligible");
        let mut ws = Workspace::for_program(&prog);
        assert!(ws.bt.eligible);
        let mut tally = Tally::default();
        let len = hay.len() as i64;
        for st in 0..=len {
            for end in st..=len {
                let want = pike(&prog, hay, st, end, flags, &mut ws);
                let mut row = vec![-1i64; ws.arena.stride];
                assert!(fits(&prog, st, end));
                match captures_at(&prog, hay, st, end, flags, &mut ws.bt, &mut row) {
                    Outcome::Match => {
                        assert_eq!(Some(&row), want.as_ref(), "{pat} [{st},{end})");
                        tally.matched += 1;
                    }
                    Outcome::NoMatch => {
                        assert_eq!(None, want, "{pat} [{st},{end})");
                        assert!(row.iter().all(|&v| v == -1), "{pat} [{st},{end}) row");
                        tally.missed += 1;
                    }
                    Outcome::WideStep => tally.wide += 1,
                }
            }
        }
        tally
    }

    /// ascii haystack: every window is served, some match, some miss
    fn same(pat: &str, flags: u8, hay: &str) {
        let t = compare_all_windows(pat, flags, hay.as_bytes());
        assert!(t.matched > 0, "{pat}: no window matched");
        assert!(t.missed > 0, "{pat}: no window missed");
        assert_eq!(t.wide, 0, "{pat}: wide step on ascii");
    }

    fn row_at(pat: &str, flags: u8, hay: &str, st: i64, end: i64) -> Vec<i64> {
        let prog = build(pat, flags);
        let mut ws = Workspace::for_program(&prog);
        let mut row = vec![-1i64; ws.arena.stride];
        let out = captures_at(&prog, hay.as_bytes(), st, end, flags, &mut ws.bt, &mut row);
        assert_eq!(out, Outcome::Match);
        row
    }

    #[test]
    fn nested_groups() {
        same("((a)(b))c", 0, "xabcab");
        assert_eq!(row_at("((a)(b))", 0, "ab", 0, 2)[2..8], [0, 2, 0, 1, 1, 2]);
    }

    #[test]
    fn alternation_priority() {
        same("(a|ab)(c|bcd)", 0, "abcd abc");
        // the window decides which branch pair can finish
        assert_eq!(row_at("(a|ab)(c|bcd)", 0, "abcd", 0, 4)[2..6], [0, 1, 1, 4]);
        assert_eq!(row_at("(a|ab)(c|bcd)", 0, "abcd", 0, 3)[2..6], [0, 2, 2, 3]);
        same("(a|ab|abc)(c|bc|)x", 0, "abcxabc");
    }

    #[test]
    fn greedy_and_lazy_quantifiers() {
        same("(a+)(a*)b", 0, "aaaab");
        same("(a+?)(a*)b", 0, "aaaab");
        same("(a*?)(a+?)(a*)b", 0, "aaaab");
        same("(a{1,3})(a{0,2}?)ab", 0, "aaaaaab");
        assert_eq!(row_at("(a+)(a*)", 0, "aaaa", 0, 4)[2..6], [0, 4, 4, 4]);
        assert_eq!(row_at("(a+?)(a*)", 0, "aaaa", 0, 4)[2..6], [0, 1, 1, 4]);
    }

    #[test]
    fn optional_group_not_participating() {
        same("(a)?(b)", 0, "abb");
        same("(?:(a)|(b))c", 0, "acbc");
        assert_eq!(row_at("(a)?(b)", 0, "b", 0, 1)[2..6], [-1, -1, 0, 1]);
        assert_eq!(row_at("(?:(a)|(b))c", 0, "bc", 0, 2)[2..6], [-1, -1, 0, 1]);
    }

    #[test]
    fn empty_matches() {
        same("(a*)", 0, "baa");
        same("(a*)*", 0, "aab");
        same("(a|)*b", 0, "aab");
        same("()|(a)", 0, "ab");
        same("(a*)+?(b?)", 0, "aabc");
    }

    #[test]
    fn group_inside_repeated_group() {
        same("(?:(a)|(b))+", 0, "abab c");
        same("((a)|(b))*c", 0, "abbac");
        same("(?:(\\d)(\\w)?,)+", 0, "1a,2,3b,");
        assert_eq!(row_at("(?:(a)|b)+", 0, "aba", 0, 3)[2..4], [2, 3]);
    }

    #[test]
    fn case_insensitive() {
        same("(He)(l+)o", RE_FLAG_I, "xHELLo hello");
        same("([a-c]+)([X-Z]*)", RE_FLAG_I, "AbCxYz.");
    }

    #[test]
    fn assertions_read_outside_the_window() {
        same("\\b(\\w+)\\b", 0, "ab cd");
        same("(\\w)\\B(\\w)", 0, "abc d");
        same("^(a+)$", 0, "aaa");
        same("^(a+)$", RE_FLAG_M, "aa\naaa\na");
        same("(a.)(.b)?", RE_FLAG_S, "a\n\nb");
        same("(a.)(.b)?", 0, "a\nab.b");
    }

    #[test]
    fn bench_pattern() {
        same("([a-z]+)=(\\d+)", 0, "kq=17;=3;abc=905;zz12;");
        assert_eq!(
            row_at("([a-z]+)=(\\d+)", 0, "abc=905;", 0, 7)[2..6],
            [0, 3, 4, 7]
        );
    }

    #[test]
    fn multi_byte_haystack_never_disagrees() {
        let hay = "a\u{e9}=1\u{6f22};\u{1f600}b";
        let mut matched = 0;
        for (pat, flags) in [
            ("(.)(.*)", 0),
            ("(.)(.*)", RE_FLAG_U),
            ("([^=]+)=(.+?)(;?)", 0),
            ("([^=]+)=(.+?)(;?)", RE_FLAG_U),
            ("(\\P{L}+)(\\p{L}*)", RE_FLAG_U),
            ("(\\w|[^a])*(.)", 0),
        ] {
            let t = compare_all_windows(pat, flags, hay.as_bytes());
            assert!(t.wide > 0, "{pat}: no window stepped wide");
            matched += t.matched;
        }
        assert!(matched > 0, "no window matched");
    }

    #[test]
    fn gate_refuses_backref_and_lookaround() {
        for pat in ["(a)\\1", "a(?=b)", "a(?!b)", "(?<=a)b", "(?<!a)b"] {
            assert!(!can_backtrack(&build(pat, 0)), "{pat}");
        }
        assert!(can_backtrack(&build("^(a|b)*?\\b.[c-d]$", 0)));
    }

    /// the wire: search with the DFA on, then the Pike pass over the
    /// window it found
    fn wired(prog: &Program, hay: &[u8], flags: u8) -> (i64, i64, Vec<i64>, Option<Vec<i64>>) {
        let r = search_from(prog, hay, 0, flags, None).expect("hit");
        let mut ws = Workspace::for_program(prog);
        let want = pike(prog, hay, r.start, r.end, flags, &mut ws);
        (r.start, r.end, r.saves().to_vec(), want)
    }

    #[test]
    fn wire_serves_inline_and_boxed_rows() {
        for (pat, stride) in [("([a-z]+)=(\\d+)", 6), ("(a)(b)(c)(d)(e)?(f|g)", 14)] {
            let mut prog = build(pat, 0);
            prog.can_dfa = true;
            let (st, end, got, want) = wired(&prog, b"..ab=12;abcdg", 0);
            assert!(fits(&prog, st, end));
            assert_eq!(got.len(), stride);
            assert_eq!(Some(got), want, "{pat}");
        }
    }

    #[test]
    fn wire_hands_a_wide_window_to_the_pike_pass() {
        let mut prog = build("(.)(.*)", 0);
        prog.can_dfa = true;
        let (st, end, got, want) = wired(&prog, "\u{e9}x".as_bytes(), 0);
        assert_eq!((st, end), (0, 3));
        assert_eq!(got[2..6], [0, 2, 2, 3]);
        assert_eq!(Some(got), want);
    }

    #[test]
    fn window_over_budget_falls_back() {
        let mut prog = build("(a|b)*(c)", 0);
        prog.can_dfa = true;
        let n = (VISITED_BITS_MAX / prog.len() + 1) as i64;
        let mut hay = vec![b'a'; n as usize];
        hay.push(b'c');
        assert!(!fits(&prog, 0, n + 1));
        let (st, end, got, want) = wired(&prog, &hay, 0);
        assert_eq!((st, end), (0, n + 1));
        assert_eq!(got[2..6], [n - 1, n, n, n + 1]);
        assert_eq!(Some(got), want);
    }

    #[test]
    fn visited_is_cleared_per_window() {
        let prog = build("(a+)(b+)", 0);
        let mut ws = Workspace::for_program(&prog);
        let mut row = vec![-1i64; ws.arena.stride];
        let mut long = vec![b'a'; 40];
        long.extend_from_slice(&[b'b'; 40]);
        let out = captures_at(&prog, &long, 0, 80, 0, &mut ws.bt, &mut row);
        assert_eq!(out, Outcome::Match);
        row.fill(-1);
        let out = captures_at(&prog, b"ab", 0, 2, 0, &mut ws.bt, &mut row);
        assert_eq!(out, Outcome::Match);
        assert_eq!(row[2..6], [0, 1, 1, 2]);
    }
}
