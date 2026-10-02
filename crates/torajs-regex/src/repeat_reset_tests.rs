//! A quantified atom's groups are unset at the top of every iteration
//! (ES §22.2.2.3.1 RepeatMatcher step 4), in every engine that reports
//! captures: the Pike VM, the bounded backtracker, the one-pass walk.

use crate::compiler::{compile, compile_rev};
use crate::onepass;
use crate::parser::Parser;
use crate::program::{Inst, Op, Program};
use crate::vm::Workspace;
use crate::vm::backtrack::{Backtrack, Outcome, captures_at};
use crate::vm::match_at::vm_match_at;
use crate::vm::match_at_rev::vm_match_at_rev;
use alloc::vec;
use alloc::vec::Vec;

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

fn resets(prog: &Program) -> Vec<(i32, i32)> {
    prog.insts
        .iter()
        .filter(|i| i.op == Op::SaveReset as u8)
        .map(|i| (i.a, i.b))
        .collect()
}

#[test]
fn only_an_atom_with_groups_resets_and_not_on_its_first_copy() {
    assert_eq!(resets(&program("(?:(a)|b)*c", false)), [(2, 4)]);
    assert_eq!(resets(&program("(?:(a)(?=(b)))+", false)), [(2, 6)]);
    assert_eq!(resets(&program("(?:(a)|b){3}", false)), [(2, 4), (2, 4)]);
    assert_eq!(resets(&program("(?:(a)|b){1,2}", false)), [(2, 4)]);
    assert_eq!(resets(&program("(?:(a)|b){0,2}", false)), [(2, 4)]);
    assert_eq!(resets(&program("x((a)|b)*", false)), [(2, 6)]);
    for pat in [r"([a-z]+)=(\d+)", "a(b+)c", "(a)(b)", "(?:ab)*"] {
        assert!(resets(&program(pat, false)).is_empty(), "{pat}");
    }
}

/// `(pattern, haystack, end, groups)`: the match from 0 ends at `end`
/// with the slots from 2 on equal to `groups`
const CASES: &[(&str, &str, i64, &[i64])] = &[
    ("(?:(a)|b)*c", "abc", 3, &[-1, -1]),
    ("(?:(a)|(b))*c", "abc", 3, &[-1, -1, 1, 2]),
    ("(?:(x)(y)?z)*w", "xyzxzw", 6, &[3, 4, -1, -1]),
    ("(?:(a)|b){2}", "ab", 2, &[-1, -1]),
    ("(?:(a)|b){2,3}", "abb", 3, &[-1, -1]),
    ("(?:(a)|b)*?c", "abc", 3, &[-1, -1]),
    ("(?:(?:(a)|b)+,)*", "a,b,", 4, &[-1, -1]),
    ("(?:(?:(a)|b)+,)*", "b,ba,", 5, &[3, 4]),
    ("(?:(a)|b\\1)*", "abab", 4, &[-1, -1]),
    ("(?:(a)|b)*", "aab", 3, &[-1, -1]),
    ("(a|b)*", "ab", 2, &[1, 2]),
];

#[test]
fn pike_backtrack_and_one_pass_unset_stale_groups() {
    for &(pat, hay, want_end, want) in CASES {
        let s = hay.as_bytes();
        let prog = program(pat, false);
        let mut ws = Workspace::for_program(&prog);
        let mut row = vec![-1i64; ws.arena.stride];
        let end = vm_match_at(&prog, s, 0, 0, &mut ws, Some(&mut row), -1);
        assert_eq!(end, want_end, "{pat}: pike end");
        assert_eq!(&row[2..], want, "{pat}: pike row");

        let mut bt = Backtrack::for_program(&prog);
        if bt.eligible {
            let mut row = vec![-1i64; ws.arena.stride];
            let got = captures_at(&prog, s, 0, end, 0, &mut bt, &mut row);
            assert_eq!(got, Outcome::Match, "{pat}: backtrack");
            assert_eq!(&row[2..], want, "{pat}: backtrack row");
        }

        if let Some(ir) = onepass::build(&program(pat, true), 0, false) {
            let mut out = vec![-1i64; ir.stride as usize];
            assert_eq!(ir.exec(s, 0, &mut out), want_end, "{pat}: one-pass");
            assert_eq!(&out[2..], want, "{pat}: one-pass row");
        }
    }
}

#[test]
fn the_reset_keeps_a_loop_one_pass() {
    for pat in [
        "(?:(a)|b)*c",
        "(?:(x)(y)?z)*w",
        "(?:(a)|b){2}",
        "(?:(a)|(b))*c",
    ] {
        let ir = onepass::build(&program(pat, true), 0, false);
        assert!(ir.is_some(), "{pat}");
    }
}

#[test]
fn a_lookbehind_body_resets_walking_leftwards() {
    // the last iteration of a reversed loop is its leftmost one
    for (hay, want) in [("abc", [0, 1]), ("bac", [-1, -1])] {
        let mut p = Parser::new(b"(?:(a)|b)*", 0);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile_rev(&mut prog, &root, 0);
        prog.emit(Inst::match_accept());
        let mut ws = Workspace::for_program(&prog);
        let mut row = vec![-1i64; ws.arena.stride];
        let end = vm_match_at_rev(&prog, hay.as_bytes(), 2, 0, &mut ws, Some(&mut row));
        assert_eq!(end, 0, "{hay}");
        assert_eq!(&row[2..4], &want, "{hay}");
    }
}
