//! Parity with the Pike VM over both payload encodings: every search
//! start over a subject answers what `vm_match_at` answers on the
//! transcoded form of the same subject, mapped back to code units.
//! Where the VM stops short of the spec (a `.` at `\r`, case folding
//! beyond ASCII, the `iu` word characters) the answer is pinned to
//! the spec instead.

use super::*;
use crate::onepass::ir::{INPUT_START, WORD_LEFT, WORD_LEFT_FOLD, WORD_RIGHT, WORD_RIGHT_FOLD};
use crate::onepass::parity::{pike, programs, transcode};
use crate::parser::{RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, RE_FLAG_U};
use crate::vm::Workspace;

fn matcher(pat: &str, flags: u8) -> Option<MatcherIr> {
    build(&programs(pat, flags).1, flags, false)
}

fn wide_matcher(pat: &str, flags: u8) -> Option<MatcherIr> {
    build(&programs(pat, flags).1, flags, true)
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

/// the end the matcher answers from 0 over `hay`
fn end_of(ir: &MatcherIr, hay: &[u16]) -> i64 {
    let mut out = vec![-1i64; ir.stride as usize];
    ir.exec(hay, 0, &mut out)
}

#[test]
fn bench_pattern() {
    same("([a-z]+)=(\\d+)", 0, "kq=17;=3;abc=905;zz12;");
    assert_eq!(agrees("([a-z]+)=(\\d+)", 0, "aaaaaaaa;bbbb="), 0);
    let ir = matcher("([a-z]+)=(\\d+)", 0).unwrap();
    // one state per seed pc past its jumps: the start, each class
    // loop, and the `=` between them
    assert_eq!(ir.states.len(), 4);
    assert_eq!(ir.stride, 6);
    assert!(
        ir.states
            .iter()
            .all(|s| s.probes == 0 && s.variants.len() == 1)
    );
    let start = &ir.states[0].variants[0];
    assert_eq!(start.edges.len(), 1);
    assert_eq!(start.edges[0].saves, [2]);
    assert!(start.accept.is_none());
    let mut out = vec![-1i64; 6];
    assert_eq!(ir.exec(b"..ab=12;", 0, &mut out), 7);
    assert_eq!(out, [2, 7, 2, 4, 5, 7]);
    assert_eq!(ir.slots_written_past_accept(), [5]);
    assert!(!ir.wide && !ir.pairs && !ir.anchored);
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
    let start = &ir.states[0].variants[0];
    assert!(start.accept.is_some() && !start.edges.is_empty());
}

#[test]
fn flags_fold_into_the_edges() {
    same("(He)(l+)o", RE_FLAG_I, "xHELLo hello");
    same("([a-c]+)([X-Z]*)", RE_FLAG_I, "AbCxYz.");
    same("(a.)(.b)?", RE_FLAG_S, "a\n\nb");
    same("(a.)(.b)?", 0, "a\nab.b");
    same("([^=;]+)=([^;]*);?", 0, "k=v;=x;ab=;q");
    same("(\\w+)@(\\w+)", RE_FLAG_U, "a@b c@d");
    same("([^a-c]+)", RE_FLAG_I, "xABcdC");
}

#[test]
fn a_latin1_literal_is_one_unit() {
    let ir = matcher("(\u{e9}+)=", 0).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(b"\xe9\xe9=22", 0, &mut out), 3);
    assert_eq!(out, [0, 3, 0, 2]);
    assert_eq!(
        wide_agrees("(\u{e9}+)=", 0, &utf16("x\u{e9}\u{e9}=22 \u{65e5}")),
        3
    );
    assert_eq!(
        wide_agrees("\u{e9}*z|\\xe8", RE_FLAG_U, &utf16("\u{e9}\u{e9}z\u{e8}")),
        4
    );
}

#[test]
fn literals_ranges_and_properties_past_latin1_take_the_code_point() {
    let s = utf16("12 H\u{e9}llo\u{65e5}\u{672c} \u{1d49c}\u{1d4b7}c 3\u{1f600}");
    for (pat, flags) in [
        ("(\\p{L}+)", RE_FLAG_U),
        ("(\\P{L}+)", RE_FLAG_U),
        ("([\\p{Lu}\\d]+)(\\p{Ll}*)", RE_FLAG_U),
        ("[^\\p{L}\\d]+", RE_FLAG_U),
        ("[^\u{4e00}-\u{9fff}]+", RE_FLAG_U),
        ("[\u{100}-\u{ffff}]+", 0),
        ("(\u{65e5}+)\u{672c}", 0),
        ("(\u{65e5}+)\u{672c}", RE_FLAG_U),
        ("\u{1d49c}+(\u{1d4b7})", RE_FLAG_U),
        ("\\u{1F600}|\\u{1d4b7}", RE_FLAG_U),
    ] {
        assert!(wide_agrees(pat, flags, &s) > 0, "{pat}");
    }
    // a lone surrogate is a code point of its own, never half a pair
    let ir = wide_matcher("[\u{10000}-\u{10ffff}]", RE_FLAG_U).unwrap();
    assert_eq!(end_of(&ir, &[0xD83D, 0xDE00]), 2);
    assert_eq!(end_of(&ir, &[0xD83D, 0x41]), -1);
    assert_eq!(end_of(&ir, &[0xDE00, 0xD83D]), -1);
    let ir = wide_matcher("\\P{L}", RE_FLAG_U).unwrap();
    assert_eq!(end_of(&ir, &[0xD83D, 0x41]), 1);
    // over Latin-1 the same class is its members up to U+00FF
    let ir = matcher("(\\p{L}+)", RE_FLAG_U).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(b"1 H\xe9llo\xf7x", 0, &mut out), 7);
    assert_eq!(out, [2, 7, 2, 7]);
    assert!(matcher("(\u{65e5}+)=", 0).is_some());
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
fn anchors_and_word_boundaries_agree_with_the_vm() {
    same("^(a+)", 0, "aab a");
    same("(a+)$", 0, "a aa");
    same("^(a+)$", 0, "aa");
    same("^", 0, "ab");
    always("$", 0, "ab");
    same("^(a+)$", RE_FLAG_M, "aa\naaa\na");
    same("^(\\w)", RE_FLAG_M, "a\nb\n\nc");
    same("(\\w)$", RE_FLAG_M, "a\nb\n");
    same("\\b(\\w+)\\b", 0, "ab cd-e");
    same("\\B(\\w)", 0, "abc d");
    same("(a)\\b", 0, "a ab");
    same("\\bword\\b", 0, "a word b xword wordy");
    same("(?:^|x)(a)", 0, "axa xa");
    same("(a)(?:$|;)", 0, "a;a a");
    same("(^)?(a)", 0, "aa");
    same("(\\w+)(?:\\b|$)", 0, "ab c");
    same("^line", RE_FLAG_M, "pre\nline-text\nend");
    same("^\\w+", 0, "hello world");
    let ir = matcher("^(a+)", 0).unwrap();
    assert_eq!(ir.states[0].probes, INPUT_START);
    assert_eq!(ir.states[0].variants.len(), 2);
    assert!(ir.states[0].variants[0].edges.is_empty());
    let ir = matcher("\\bword\\b", 0).unwrap();
    assert_eq!(ir.states[0].probes, WORD_LEFT | WORD_RIGHT);
    assert_eq!(ir.states[0].variants.len(), 4);
}

#[test]
fn line_starts_and_ends_follow_the_spec_line_terminators() {
    // the Pike VM knows `\n` only, so these are pinned
    let ir = matcher("^(b)", RE_FLAG_M).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(b"a\rb", 0, &mut out), 3);
    assert_eq!(ir.exec(b"a b", 0, &mut out), -1);
    let ir = matcher("(a)$", RE_FLAG_M).unwrap();
    assert_eq!(ir.exec(b"a\rb", 0, &mut out), 1);
    let ir = wide_matcher("^(b)", RE_FLAG_M).unwrap();
    assert_eq!(end_of(&ir, &utf16("a\u{2028}b")), 3);
    assert_eq!(end_of(&ir, &utf16("a\u{2027}b")), -1);
}

#[test]
fn case_folding_beyond_ascii_follows_canonicalize() {
    // the Pike VM folds ASCII only, so these are pinned
    let ir = matcher("([\u{e9}])", RE_FLAG_I).unwrap();
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(b"x\xc9", 0, &mut out), 2);
    assert_eq!(ir.exec(b"x\xc9", 0, &mut out), 2);
    assert_eq!(out, [1, 2, 1, 2]);
    let ir = matcher("([^\u{e9}])", RE_FLAG_I).unwrap();
    assert_eq!(ir.exec(b"\xc9\xe9x", 0, &mut out), 3);
    let ir = matcher("([\u{ff}])", RE_FLAG_I).unwrap();
    assert_eq!(ir.exec(b"\xff", 0, &mut out), 1);
    // ß is its own class without u; ẞ folds to it under u
    let ir = wide_matcher("([\u{df}])", RE_FLAG_I).unwrap();
    assert_eq!(end_of(&ir, &utf16("\u{1e9e}")), -1);
    let ir = wide_matcher("([\u{df}])", RE_FLAG_I | RE_FLAG_U).unwrap();
    assert_eq!(end_of(&ir, &utf16("\u{1e9e}")), 1);
    // µ meets Μ and μ in both modes, ÿ meets Ÿ
    for flags in [RE_FLAG_I, RE_FLAG_I | RE_FLAG_U] {
        let ir = wide_matcher("([\u{b5}])", flags).unwrap();
        assert_eq!(end_of(&ir, &utf16("\u{39c}")), 1);
        assert_eq!(end_of(&ir, &utf16("\u{3bc}")), 1);
        assert_eq!(end_of(&ir, &utf16("\u{3bd}")), -1);
        let ir = wide_matcher("([\u{ff}])", flags).unwrap();
        assert_eq!(end_of(&ir, &utf16("\u{178}")), 1);
    }
    // ſ and Kelvin K fold into ASCII only under u
    let ir = wide_matcher("(s)", RE_FLAG_I).unwrap();
    assert_eq!(end_of(&ir, &utf16("\u{17f}")), -1);
    let ir = wide_matcher("(s)", RE_FLAG_I | RE_FLAG_U).unwrap();
    assert_eq!(end_of(&ir, &utf16("\u{17f}")), 1);
    let ir = wide_matcher("([^k])", RE_FLAG_I | RE_FLAG_U).unwrap();
    assert_eq!(end_of(&ir, &utf16("\u{212a}")), -1);
    let ir = wide_matcher("(\\w)", RE_FLAG_I | RE_FLAG_U).unwrap();
    assert_eq!(end_of(&ir, &utf16("\u{212a}")), 1);
    let ir = wide_matcher("(\\w)", RE_FLAG_I).unwrap();
    assert_eq!(end_of(&ir, &utf16("\u{212a}")), -1);
    // the iu word characters reach `\b`
    let ir = wide_matcher("\\b(.)", RE_FLAG_I | RE_FLAG_U).unwrap();
    assert_eq!(ir.states[0].probes, WORD_LEFT_FOLD | WORD_RIGHT_FOLD);
    assert_eq!(end_of(&ir, &utf16(" \u{17f}")), 2);
    let ir = wide_matcher("\\b(.)", RE_FLAG_I).unwrap();
    assert_eq!(end_of(&ir, &utf16(" \u{17f}")), -1);
}

#[test]
fn a_sticky_matcher_tries_the_start_only() {
    let ir = matcher("(a)", RE_FLAG_Y).unwrap();
    assert!(ir.anchored);
    let mut out = vec![-1i64; 4];
    assert_eq!(ir.exec(b"ba", 0, &mut out), -1);
    assert_eq!(ir.exec(b"ba", 1, &mut out), 2);
    assert_eq!(out, [1, 2, 1, 2]);
    same("(\\d+)", RE_FLAG_Y, "ab12");
    same("^(a)", RE_FLAG_Y, "aa");
    always("(a*)", RE_FLAG_Y, "baa");
}

#[test]
fn the_arms_of_an_alternation_exit_into_one_state() {
    // `+` is one mandatory copy and a looped one: the start, the loop
    // every arm of both rejoins, and after `a` and after `c` in each
    let m = wide_matcher("(?:ab|cd|e)+", 0).expect("one-pass");
    assert_eq!(m.states.len(), 6);
    for hay in ["abcde", "eab", "abc", "xcdx"] {
        same("(?:ab|cd|e)+", 0, hay);
    }
    // both arms claim `a` and rejoin with the same slots
    same("(?:^a|a)b", 0, "ab xab");
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
        ("(\u{65e5}+)=", RE_FLAG_I),
        ("[\u{4e2d}]", RE_FLAG_I),
        ("(\\p{L}+)", RE_FLAG_U | RE_FLAG_I),
        ("(?:^a|(a))b", 0),
    ] {
        assert!(matcher(pat, flags).is_none(), "{pat}");
        assert!(wide_matcher(pat, flags).is_none(), "{pat} wide");
    }
}
