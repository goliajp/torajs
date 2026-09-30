//! Parity with the Pike VM over both payload encodings: every search
//! start over a subject answers what `vm_match_at` answers on the
//! transcoded form of the same subject, mapped back to code units.

use super::*;
use crate::compiler::compile;
use crate::parser::{Parser, RE_FLAG_I, RE_FLAG_S, RE_FLAG_U};
use crate::vm::Workspace;
use crate::vm::match_at::vm_match_at;

/// the program the Pike VM runs (UTF-8 haystack) and the one the
/// matcher is built from (code units); they differ only where a
/// class is unsafe for the byte walk
fn programs(pat: &str, flags: u8) -> (Program, Program) {
    let mut p = Parser::new(pat.as_bytes(), flags);
    let root = p.parse().expect("parse failed");
    let mut vm = Program::new();
    compile(&mut vm, &root, flags);
    vm.emit(Inst::match_accept());
    vm.has_save = vm.any_save();
    vm.finalize_backref_caps();
    let mut raw = Program::new();
    raw.raw_classes = true;
    compile(&mut raw, &root, flags);
    raw.emit(Inst::match_accept());
    (vm, raw)
}

fn matcher(pat: &str, flags: u8) -> Option<MatcherIr> {
    build(&programs(pat, flags).1, flags, false)
}

fn wide_matcher(pat: &str, flags: u8) -> Option<MatcherIr> {
    build(&programs(pat, flags).1, flags, true)
}

/// the Pike VM's leftmost-first answer from byte `from`: start, end, row
fn pike(prog: &Program, s: &[u8], from: usize, flags: u8) -> Option<(i64, i64, Vec<i64>)> {
    let mut ws = Workspace::for_program(prog);
    let mut st = from;
    while st <= s.len() {
        let mut row = vec![-1i64; ws.arena.stride];
        let end = vm_match_at(prog, s, st as i64, flags, &mut ws, Some(&mut row), -1);
        if end >= 0 {
            row[0] = st as i64;
            row[1] = end;
            return Some((st as i64, end, row));
        }
        // the VM scans code points of its UTF-8 form
        st += 1;
        while st < s.len() && s[st] & 0xC0 == 0x80 {
            st += 1;
        }
    }
    None
}

/// The runtime's transcoding of a UTF-16 payload (`str_helpers`):
/// WTF-8 code units, or code points with the pairs merged under `u`.
/// Also the unit index of every byte offset the VM can answer.
fn transcode(units: &[u16], merge: bool) -> (Vec<u8>, Vec<i64>) {
    let mut out = Vec::new();
    let mut unit_of = Vec::new();
    let mut i = 0;
    while i < units.len() {
        let cu = units[i] as u32;
        let (cp, w) = if merge
            && (0xD800..=0xDBFF).contains(&cu)
            && i + 1 < units.len()
            && (0xDC00..=0xDFFF).contains(&(units[i + 1] as u32))
        {
            (
                0x10000 + ((cu - 0xD800) << 10) + (units[i + 1] as u32 - 0xDC00),
                2,
            )
        } else {
            (cu, 1)
        };
        let mut buf = [0u8; 4];
        let n = crate::utf8::utf8_encode_cp(cp as i32, &mut buf);
        for k in 0..n {
            unit_of.push(if k == 0 { i as i64 } else { -1 });
            out.push(buf[k]);
        }
        i += w;
    }
    unit_of.push(units.len() as i64);
    (out, unit_of)
}

fn assert_row_matches(pat: &str, from: usize, end: i64, out: &[i64], want: (i64, i64, Vec<i64>)) {
    let (st, wend, row) = want;
    assert_eq!(end, wend, "{pat} from {from}: end");
    assert_eq!(out[0], st, "{pat} from {from}: start");
    assert_eq!(out, row, "{pat} from {from}: row");
}

/// every search start over `hay` answers exactly what the Pike VM
/// answers; returns how many of them hit
fn agrees(pat: &str, flags: u8, hay: &str) -> usize {
    let (prog, raw) = programs(pat, flags);
    let ir = build(&raw, flags, false).unwrap_or_else(|| panic!("{pat}: not one-pass"));
    assert_eq!(
        ir.stride as usize,
        Workspace::for_program(&prog).arena.stride
    );
    let s = hay.as_bytes();
    let mut hits = 0;
    for from in 0..=s.len() {
        let want = pike(&prog, s, from, flags);
        let mut out = vec![-1i64; ir.stride as usize];
        let end = ir.exec(s, from, &mut out);
        match want {
            Some(w) => {
                assert_row_matches(pat, from, end, &out, w);
                hits += 1;
            }
            None => assert_eq!(end, -1, "{pat} from {from}: expected miss"),
        }
    }
    hits
}

/// the wide walk over `hay` against the Pike VM over its transcoded
/// form, from every unit that starts a code point
fn wide_agrees(pat: &str, flags: u8, hay: &[u16]) -> usize {
    let (prog, raw) = programs(pat, flags);
    let ir = build(&raw, flags, true).unwrap_or_else(|| panic!("{pat}: not one-pass"));
    let (bytes, unit_of) = transcode(hay, unicode_mode(flags));
    let mut hits = 0;
    for (b, &u) in unit_of.iter().enumerate() {
        if u < 0 {
            continue;
        }
        let from = u as usize;
        let want = pike(&prog, &bytes, b, flags).map(|(st, end, row)| {
            let map = |v: i64| if v < 0 { v } else { unit_of[v as usize] };
            (map(st), map(end), row.into_iter().map(map).collect())
        });
        let mut out = vec![-1i64; ir.stride as usize];
        let end = ir.exec(hay, from, &mut out);
        match want {
            Some(w) => {
                assert_row_matches(pat, from, end, &out, w);
                hits += 1;
            }
            None => assert_eq!(end, -1, "{pat} from {from}: expected miss"),
        }
    }
    hits
}

/// some search starts hit, some miss
fn same(pat: &str, flags: u8, hay: &str) {
    let hits = agrees(pat, flags, hay);
    assert!(hits > 0, "{pat}: nothing matched in {hay:?}");
    assert!(
        hits <= hay.len(),
        "{pat}: no search start missed in {hay:?}"
    );
}

/// every search start hits (a pattern that matches empty)
fn always(pat: &str, flags: u8, hay: &str) {
    assert_eq!(agrees(pat, flags, hay), hay.len() + 1, "{pat} over {hay:?}");
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[test]
fn bench_pattern() {
    same("([a-z]+)=(\\d+)", 0, "kq=17;=3;abc=905;zz12;");
    assert_eq!(agrees("([a-z]+)=(\\d+)", 0, "aaaaaaaa;bbbb="), 0);
    let ir = matcher("([a-z]+)=(\\d+)", 0).unwrap();
    // one state per seed pc: the loop body's class and the first
    // one share a closure shape but not a seed
    assert_eq!(ir.states.len(), 6);
    assert_eq!(ir.stride, 6);
    assert_eq!(ir.states[0].edges.len(), 1);
    assert_eq!(ir.states[0].edges[0].saves, [2]);
    assert!(ir.states[0].accept.is_none());
    let mut out = vec![-1i64; 6];
    assert_eq!(ir.exec(b"..ab=12;", 0, &mut out), 7);
    assert_eq!(out, [2, 7, 2, 4, 5, 7]);
    assert_eq!(ir.slots_written_past_accept(), [5]);
    assert!(!ir.wide && !ir.pairs);
}

#[test]
fn groups_alternation_and_optionals() {
    same("(a)?(b)", 0, "abb");
    same("(?:(a)|(b))c", 0, "acbc");
    same("(a)|(b)", 0, "xba");
    same("(cat|dog)s?", 0, "cats dog cow");
    same("((a)(b))c", 0, "xabcab");
    same("(?:(a)|b)+", 0, "abab c");
    same("(?:(\\d)(\\w)?,)+", 0, "1a,2,3b,");
    same("(x)(y)(z)(w)", 0, "xyzw");
}

#[test]
fn empty_matches_and_loops() {
    always("(a*)", 0, "baa");
    always("(a*)*", 0, "aab");
    same("(a|)*b", 0, "aab");
    always("()|(a)", 0, "ab");
    always("x*", 0, "yxx");
    always("(a*?)", 0, "aa");
    always("(a+?)?", 0, "aa");
    let ir = matcher("(a*)", 0).unwrap();
    assert!(ir.states[0].accept.is_some() && !ir.states[0].edges.is_empty());
}

#[test]
fn flags_fold_into_the_edges() {
    same("(He)(l+)o", RE_FLAG_I, "xHELLo hello");
    same("([a-c]+)([X-Z]*)", RE_FLAG_I, "AbCxYz.");
    same("(a.)(.b)?", RE_FLAG_S, "a\n\nb");
    same("(a.)(.b)?", 0, "a\nab.b");
    same("([^=;]+)=([^;]*);?", 0, "k=v;=x;ab=;q");
    same("(\\w+)@(\\w+)", RE_FLAG_U, "a@b c@d");
}

#[test]
fn latin1_units_take_the_negated_class() {
    let ir = matcher("([^a]+)", 0).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(b"a\xe9\xffa", 0, &mut out), 3);
    assert_eq!(out, [1, 3, 1, 3]);
    let ir = matcher("(a)", RE_FLAG_I).unwrap();
    assert_eq!(ir.exec(b"\xc1\xe1A", 0, &mut out), 3);
    let ir = matcher("([\u{e9}-\u{ff}]+)", 0).unwrap();
    assert_eq!(ir.exec(b"a\xe9\xffa", 0, &mut out), 3);
    assert_eq!(out, [1, 3, 1, 3]);
    assert_eq!(ir.exec(b"\xe8", 0, &mut out), -1);
}

#[test]
fn a_dot_skips_every_line_terminator() {
    // §12.3 LineTerminator: the Pike VM's `.` stops at `\n` only, so
    // these are pinned to the spec rather than compared
    let ir = matcher("a(.)c", 0).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(b"a\rc", 0, &mut out), -1);
    assert_eq!(ir.exec(b"a\nc", 0, &mut out), -1);
    assert_eq!(ir.exec(b"a\x85c", 0, &mut out), 3);
    let ir = wide_matcher("a(.)c", 0).unwrap();
    assert_eq!(ir.exec(&utf16("a\u{2028}c"), 0, &mut out), -1);
    assert_eq!(ir.exec(&utf16("a\u{2029}c"), 0, &mut out), -1);
    assert_eq!(ir.exec(&utf16("a\u{2027}c"), 0, &mut out), 3);
    let ir = wide_matcher("a(.)c", RE_FLAG_S).unwrap();
    assert_eq!(ir.exec(&utf16("a\u{2028}c"), 0, &mut out), 3);
}

#[test]
fn wide_units_above_latin1_take_only_the_edges_that_admit_them() {
    let s = utf16("a\u{4e2d}\u{1F600}b");
    assert!(wide_agrees("([^a]+)", 0, &s) > 0);
    assert!(wide_agrees("([^a]+)", RE_FLAG_U, &s) > 0);
    assert!(wide_agrees("(.)(.)", 0, &s) > 0);
    assert!(wide_agrees("(.)(.)", RE_FLAG_U, &s) > 0);
    assert!(wide_agrees("([a-z]+)", 0, &s) > 0);
    assert!(wide_agrees("(\\w+)", RE_FLAG_U, &s) > 0);
    assert!(wide_agrees("(\\W)", RE_FLAG_U, &s) > 0);
    assert!(wide_agrees("([\u{e9}-\u{ff}]+)", 0, &utf16("a\u{e9}\u{ff}\u{100}")) > 0);
    let ir = wide_matcher("([a-z]+)", 0).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(&s, 0, &mut out), 1);
    assert_eq!(ir.exec(&s, 1, &mut out), 5);
    assert_eq!(out, [4, 5, 4, 5]);
    assert_eq!(ir.exec(&utf16("\u{4e2d}"), 0, &mut out), -1);
}

#[test]
fn a_surrogate_pair_is_one_step_under_u_and_two_units_otherwise() {
    let s = utf16("\u{1F600}x");
    let ir = wide_matcher("(.)", 0).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(&s, 0, &mut out), 1);
    let ir = wide_matcher("(.)", RE_FLAG_U).unwrap();
    assert!(ir.pairs);
    assert_eq!(ir.exec(&s, 0, &mut out), 2);
    assert_eq!(out, [0, 2, 0, 2]);
    // a scan that misses the pair's start steps over the whole pair
    let ir = wide_matcher("(x)", RE_FLAG_U).unwrap();
    assert_eq!(ir.exec(&s, 0, &mut out), 3);
    assert_eq!(out, [2, 3, 2, 3]);
    // a lone surrogate is one unit either way
    let lone = [0xD83Du16, 0x78];
    assert!(wide_agrees("(.)(x)", RE_FLAG_U, &lone) > 0);
    assert!(wide_agrees("(.)(x)", 0, &lone) > 0);
    let lo_first = [0xDE00u16, 0xD83D, 0x78];
    assert!(wide_agrees("([^x]+)", RE_FLAG_U, &lo_first) > 0);
    assert!(wide_agrees("(..)x", 0, &lo_first) > 0);
}

#[test]
fn refuses_what_it_cannot_walk() {
    for (pat, flags) in [
        ("(a|ab)(c|bcd)", 0),
        ("(a+)(a*)b", 0),
        ("\\d|\\d\\d", 0),
        ("(a)\\1", 0),
        ("a(?=b)", 0),
        ("(?<!a)b", 0),
        ("^(a)", 0),
        ("(a)$", 0),
        ("\\b(\\w+)", 0),
        ("(\u{e9}+)=", 0),
        ("[\u{4e2d}]", 0),
        ("(\\p{L}+)", RE_FLAG_U),
    ] {
        assert!(matcher(pat, flags).is_none(), "{pat}");
        assert!(wide_matcher(pat, flags).is_none(), "{pat} wide");
    }
}
