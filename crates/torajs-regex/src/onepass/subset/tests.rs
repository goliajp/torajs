//! Parity of the leftmost-first DFA with the Pike VM: every search
//! start over a subject answers what `vm_match_at` answers.

use super::*;
use crate::onepass::parity::{pike, programs, transcode};
use crate::parser::{RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, RE_FLAG_U};

const MAX: usize = 256;

fn dfa(pat: &str, flags: u8, wide: bool) -> MatcherIr {
    build_subset(&programs(pat, flags).1, flags, wide, MAX)
        .unwrap_or_else(|| panic!("{pat}: no subset matcher"))
}

/// every start over `hay` answers the VM's span; returns the hits
fn agrees(pat: &str, flags: u8, hay: &str) -> usize {
    let (prog, _) = programs(pat, flags);
    let ir = dfa(pat, flags, false);
    assert_eq!(ir.stride, 2);
    let s = hay.as_bytes();
    let mut hits = 0;
    for from in 0..=s.len() {
        let mut out = vec![-1i64; 2];
        let end = ir.exec(s, from, &mut out);
        match pike(&prog, s, from, flags) {
            Some((st, wend, _)) => {
                assert_eq!((out[0], end), (st, wend), "{pat} over {hay:?} from {from}");
                hits += 1;
            }
            None => assert_eq!(end, -1, "{pat} over {hay:?} from {from}: expected miss"),
        }
    }
    hits
}

/// the wide walk against the VM over the transcoded subject
fn wide_agrees(pat: &str, flags: u8, hay: &[u16]) -> usize {
    let (prog, _) = programs(pat, flags);
    let ir = dfa(pat, flags, true);
    let (bytes, unit_of) = transcode(hay, unicode_mode(flags));
    let mut hits = 0;
    for (b, &u) in unit_of.iter().enumerate() {
        if u < 0 {
            continue;
        }
        let mut out = vec![-1i64; 2];
        let end = ir.exec(hay, u as usize, &mut out);
        match pike(&prog, &bytes, b, flags) {
            Some((st, wend, _)) => {
                let want = (unit_of[st as usize], unit_of[wend as usize]);
                assert_eq!((out[0], end), want, "{pat} from unit {u}");
                hits += 1;
            }
            None => assert_eq!(end, -1, "{pat} from unit {u}: expected miss"),
        }
    }
    hits
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[test]
fn greedy_and_lazy_dots_end_at_the_last_accept() {
    let hay = "pre a\nmiddle\nc post c9 ac a\nc";
    for pat in [
        "a.+c", "a.*c", "a.+?c", "a.*?c", ".+", ".*", "a.+c.", "a.{2,4}c",
    ] {
        assert!(agrees(pat, RE_FLAG_S, hay) > 0, "{pat}");
    }
    for pat in ["a.+c", "a.*?c", "x.+c"] {
        agrees(pat, 0, "a\nbc a bc axc");
        assert_eq!(agrees(pat, RE_FLAG_S, "zzz"), 0);
    }
    let mut out = [-1i64; 2];
    let ir = dfa("a.+c", RE_FLAG_S, false);
    assert_eq!(ir.exec(b"pre a\nmiddle\nc post c", 0, &mut out), 21);
    assert_eq!(out[0], 4);
    let ir = dfa("a.+?c", RE_FLAG_S, false);
    assert_eq!(ir.exec(b"pre a\nmiddle\nc post c", 0, &mut out), 14);
}

#[test]
fn a_dot_under_s_takes_every_line_terminator() {
    let mut out = [-1i64; 2];
    let ir = dfa("a.+c", RE_FLAG_S, false);
    assert_eq!(ir.exec(b"a\r\nc", 0, &mut out), 4);
    let ir = dfa("a.+c", 0, false);
    assert_eq!(ir.exec(b"a\rc", 0, &mut out), -1);
    assert_eq!(ir.exec(b"a\x85c", 0, &mut out), 3);
    for flags in [RE_FLAG_S, RE_FLAG_S | RE_FLAG_U] {
        let ir = dfa("a.+c", flags, true);
        assert_eq!(ir.exec(&utf16("a\u{2028}\u{2029}c"), 0, &mut out), 4);
    }
    let ir = dfa("a.+c", 0, true);
    assert_eq!(ir.exec(&utf16("a\u{2028}c"), 0, &mut out), -1);
}

#[test]
fn leftmost_first_prefers_the_earlier_alternative() {
    assert!(agrees("a|ab", 0, "ab xab") > 0);
    assert!(agrees("\\d|\\d\\d", 0, "x12 3") > 0);
    assert!(agrees("(?:a|ab)(?:c|bcd)", 0, "abcd abc acd") > 0);
    assert!(agrees("\\w+\\d", 0, "abc1 22 x") > 0);
    assert!(agrees("(?:ab)*a", RE_FLAG_I, "ABABa aB") > 0);
    assert!(agrees("x*", 0, "axxb") > 0);
}

#[test]
fn assertions_select_a_variant_of_the_subset() {
    assert!(agrees("^.*b", RE_FLAG_M | RE_FLAG_S, "ab\nxb\nc") > 0);
    assert!(agrees("\\b\\w+\\b.*z", 0, "ab cd z ef") > 0);
    assert!(agrees(".+$", 0, "ab\ncd") > 0);
    assert!(agrees("^a.*c$", RE_FLAG_M, "abc\nac\nab") > 0);
}

#[test]
fn a_pair_is_one_dot_under_u() {
    let s = utf16("a\u{1F600}\u{4e2d}c\nz\u{1F600}c");
    assert!(wide_agrees("a.+c", RE_FLAG_S | RE_FLAG_U, &s) > 0);
    assert!(wide_agrees("a.+?c", RE_FLAG_S | RE_FLAG_U, &s) > 0);
    assert!(wide_agrees(".{2}c", RE_FLAG_S | RE_FLAG_U, &s) > 0);
    let mut out = [-1i64; 2];
    let ir = dfa("a.c", RE_FLAG_U, true);
    assert_eq!(ir.exec(&utf16("a\u{1F600}c"), 0, &mut out), 4);
    let ir = dfa("a.c", 0, true);
    assert_eq!(ir.exec(&utf16("a\u{1F600}c"), 0, &mut out), -1);
    let ir = dfa("a..c", 0, true);
    assert_eq!(ir.exec(&utf16("a\u{1F600}c"), 0, &mut out), 4);
}

#[test]
fn a_sticky_dfa_tries_the_start_only() {
    let mut out = [-1i64; 2];
    let ir = dfa("a.+c", RE_FLAG_S | RE_FLAG_Y, false);
    assert!(ir.anchored);
    assert_eq!(ir.exec(b"xabc", 0, &mut out), -1);
    assert_eq!(ir.exec(b"xabc", 1, &mut out), 4);
}

#[test]
fn refuses_captures_lookaround_and_blowup() {
    let built = |pat: &str| build_subset(&programs(pat, 0).1, 0, false, MAX);
    assert!(built("(a).+c").is_none());
    assert!(built("a(?=b)").is_none());
    assert!(built("a.+c").is_some());
    assert!(built("(?:a|b)*a(?:a|b){9}").is_none());
}
