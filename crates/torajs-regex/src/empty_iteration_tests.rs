//! An iteration past a quantifier's minimum that ends where it began
//! fails (ES §22.2.2.3.1 RepeatMatcher step 2.b), in every engine that
//! walks the loop: the Pike VM with and without backreferences, the
//! bounded backtracker, the one-pass walk.

use crate::compiler::compile;
use crate::onepass;
use crate::parser::Parser;
use crate::program::{Inst, Op, Program};
use crate::vm::Workspace;
use crate::vm::backtrack::{Backtrack, Outcome, captures_at};
use crate::vm::match_at::vm_match_at;
use alloc::vec;

fn program(pat: &str, raw: bool) -> Program {
    let mut p = Parser::new(pat.as_bytes(), 0);
    let root = p.parse().expect("parse failed");
    let mut prog = Program::new();
    prog.raw_classes = raw;
    compile(&mut prog, &root, 0);
    prog.emit(Inst::match_accept());
    prog.has_save = prog.any_save();
    prog.finalize_backref_caps();
    prog
}

fn checks(pat: &str) -> usize {
    let prog = program(pat, false);
    let count = |op: Op| prog.insts.iter().filter(|i| i.op == op as u8).count();
    assert_eq!(count(Op::IterStart), count(Op::IterCheck), "{pat}");
    count(Op::IterCheck)
}

#[test]
fn only_iterations_past_the_minimum_of_an_atom_that_can_match_empty_check() {
    assert_eq!(checks("(?:a|)*"), 1);
    assert_eq!(checks("(?:a?){1,3}"), 2);
    assert_eq!(checks("(?:a?){2,}"), 1);
    assert_eq!(checks("(?:a*)*"), 1);
    assert_eq!(checks("(?:(?=a))*"), 1);
    assert_eq!(checks("(a)(?:\\1|b)*"), 1);
    for pat in ["(?:a|b)*", "(?:a?){2}", "a*", "(?:ab?)+", "[a-z]{0,3}"] {
        assert_eq!(checks(pat), 0, "{pat}");
    }
}

/// `(pattern, haystack, start, end, groups)`: the match from `start`
/// ends at `end` (`-1`: none) with the slots from 2 on equal to `groups`
const CASES: &[(&str, &str, i64, i64, &[i64])] = &[
    ("(?:(a)|){0,3}", "a", 0, 1, &[0, 1]),
    ("(?:(a)|){0,3}", "aa", 0, 2, &[1, 2]),
    ("(?:(a)|){0,3}?b", "ab", 0, 2, &[0, 1]),
    ("(?:|a){0,2}", "aa", 0, 2, &[]),
    ("(?:|a)*", "aa", 0, 2, &[]),
    ("(?:(a)|)*", "a", 0, 1, &[0, 1]),
    ("(?:(a)|){2,}", "a", 0, 1, &[-1, -1]),
    ("(?:(a)|)*\\1b", "ab", 0, -1, &[-1, -1]),
    ("(?:(a)|)*\\1b", "ab", 1, 2, &[-1, -1]),
    ("(?:(a?))*\\1b", "ab", 1, 2, &[-1, -1]),
    ("(?:(a)|)*?\\1b", "ab", 1, 2, &[-1, -1]),
    ("(?:(a)|\\2)*(x)?", "a", 0, 1, &[0, 1, -1, -1]),
];

#[test]
fn pike_backtrack_and_one_pass_fail_an_empty_iteration() {
    for &(pat, hay, st, want_end, want) in CASES {
        let s = hay.as_bytes();
        let prog = program(pat, false);
        let mut ws = Workspace::for_program(&prog);
        let mut row = vec![-1i64; ws.arena.stride];
        let end = vm_match_at(&prog, s, st, 0, &mut ws, Some(&mut row), -1);
        assert_eq!(end, want_end, "{pat} @{st}: pike end");
        if end < 0 {
            continue;
        }
        assert_eq!(&row[2..], want, "{pat} @{st}: pike row");

        let mut bt = Backtrack::for_program(&prog);
        if bt.eligible {
            let mut row = vec![-1i64; ws.arena.stride];
            let got = captures_at(&prog, s, st, end, 0, &mut bt, &mut row);
            assert_eq!(got, Outcome::Match, "{pat}: backtrack");
            assert_eq!(&row[2..], want, "{pat}: backtrack row");
        }

        if let Some(ir) = onepass::build(&program(pat, true), 0, false) {
            let mut out = vec![-1i64; ir.stride as usize];
            assert_eq!(
                ir.exec(s, st as usize, &mut out),
                want_end,
                "{pat}: one-pass"
            );
            assert_eq!(&out[2..], want, "{pat}: one-pass row");
        }
    }
}
