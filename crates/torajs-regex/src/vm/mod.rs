//! Pike-style NFA matcher — port of `runtime_regex.c` L1554-2122.
//!
//! Russ-Cox-style virtual machine: per input position, advance every
//! currently-active thread one CHAR / ANYCHAR / CLASS / BACKREF step;
//! threads waiting on epsilon ops (JMP, SPLIT, SAVE, ANCHOR_B/E,
//! WBOUND/NWBOUND, LOOKAHEAD/LOOKBEHIND) resolve immediately and
//! enqueue the resulting thread state into the same step.
//!
//! Leftmost-first semantics: when MATCH fires for a thread at
//! position `p`, lower-priority threads in cur at this step are
//! dead (can't beat it), but higher-priority threads already
//! advanced into nxt can still extend the match by consuming more
//! chars. The latest MATCH seen wins.
//!
//! ## Module split (each ≤ 500 LOC HARD RULE)
//!
//! - [`mod@self`] — [`Thread`] / [`ThreadList`] / [`VisitedTable`] /
//!   [`Workspace`] data structures + [`char_eq`].
//! - [`search`] — public entry points [`search_from`] /
//!   [`search_from_with_ws`] / [`match_anchor`] (outer scan loop:
//!   prefix anchor + DFA fast path + Pike-VM fallback).
//! - [`dispatch`] — `add_thread` (epsilon expansion) + `sub_probe`
//!   (lookahead) + `sub_probe_rev` (lookbehind, reverse-compiled).
//! - [`match_at`] — `vm_match_at` inner loop (operand dispatch on
//!   the input-consuming op set).
//! - [`match_at_rev`] — `vm_match_at_rev` reverse inner loop
//!   (lookbehind bodies, reverse-compiled; ES §22.2.2 MatchReverse).

pub mod dispatch;
pub mod match_at;
pub mod match_at_rev;
pub mod result;
pub mod saves_arena;
pub mod search;

pub use result::{MatchResult, save_slot};
pub use saves_arena::SavesArena;
use saves_arena::detect_stride;
pub use search::{match_anchor, search_from, search_from_with_ws};

use crate::parser::RE_FLAG_I;
use crate::program::Program;
use alloc::{vec, vec::Vec};

/// Per-thread state in the Pike NFA. Each step the matcher iterates
/// every Thread in `cur` and advances PCs to `nxt` (or, for backref
/// continuation / u-flag deferred bytes, back to `nxt` at the same
/// `pc` with mutated bookkeeping).
///
/// V0.2 P14-S12 — saves arena. Pre-S12 the capture-save slots lived
/// inline (`[i64; 64]` = 512 bytes/Thread → 528-byte Thread); every
/// `nxt.push(Thread{..})` in the hot loop memcpy'd the whole 528
/// bytes into the `Vec<Thread>` backing storage. saves now live in a
/// `SavesArena` owned by [`Workspace`]; Thread carries a 4-byte
/// `saves_id` handle. Thread shrinks from 528 → 24 bytes (22×); the
/// hot-loop nxt.push memcpy collapses from 528 → 24 bytes. `Op::Save`
/// pays one arena `alloc_clone` (still 512 bytes copy) per capture-
/// boundary epsilon, but those fire ~O(1) per match versus the
/// per-input-byte Thread copy that S10 borrow-split could not eliminate.
#[derive(Clone, Copy, Debug)]
pub struct Thread {
    /// Program counter (index into `Program.insts`).
    pub pc: usize,
    /// Byte progress within an in-flight `OP_BACKREF` evaluation
    /// (0..cap_len). 0 = fresh entry / not in a backref.
    pub br_offset: i32,
    /// Outer-step defer counter for `OP_ANYCHAR` / `OP_CLASS` under
    /// the u flag with a multi-byte code point at the consume site.
    /// Bypasses the visited table so deferred threads survive
    /// step-to-step swaps without colliding with fresh entrants.
    pub u_skip: i32,
    /// Handle into the Workspace `SavesArena` — identifies the row of
    /// Handle of this thread's stride-sized capture-saves row.
    /// SPLIT forks each get a fresh `alloc_clone` so SAVE in one
    /// branch doesn't leak into the other.
    pub saves_id: u32,
}

/// Linked-list-replacement: `Vec<Thread>` with a `step_id` stamp
/// used by [`VisitedTable`] to dedup PCs *within* a step. (Across
/// steps the bitmap "auto-resets" by mismatching step_id — no clear
/// pass needed.)
#[derive(Debug)]
pub struct ThreadList {
    pub list: Vec<Thread>,
    pub step_id: u32,
}

impl ThreadList {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            list: Vec::with_capacity(cap),
            step_id: 0,
        }
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn push(&mut self, t: Thread) {
        self.list.push(t);
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// Per-PC visited stamp. `visited[pc] == step_id` ⇒ that PC was
/// already enqueued *this* step; skip the duplicate (first-write-
/// wins matches Pike-NFA leftmost-priority).
#[derive(Debug)]
pub struct VisitedTable {
    pub visited: Vec<u32>,
}

impl VisitedTable {
    pub fn with_size(n: usize) -> Self {
        Self {
            visited: vec![0u32; n],
        }
    }
}

/// True iff `prog` emits any `Op::Save`. Used by the DFA fast path
/// (chunk 9) to skip the second-pass Pike VM on patterns whose
/// captures are trivially all-`-1`. Reads the `Program::has_save`
/// flag set once at compile time (`regex/compile.rs`) — chunk 7.7
/// v2 step 12 C2 Phase B-1 attack #J eliminated the prior per-call
/// O(N) linear scan over `prog.insts`, which after chunk-10d
/// `utf8_class_expand` had become ~200 ns/iter (13% of per-iter
/// budget) on `/\p{L}+/u`-class patterns.
#[inline]
fn prog_has_save(prog: &Program) -> bool {
    prog.has_save
}

/// Reusable per-Program workspace — allocates once at search start;
/// re-used across tight-loop iterations (replaceAll / matchAll /
/// split) via [`search_from_with_ws`]. Sized so `cur/nxt` lists and
/// the saves arena each pre-reserve room for `n_insts` rows; growth
/// happens on demand via `Vec::resize`.
#[derive(Debug)]
pub struct Workspace {
    pub cur: ThreadList,
    pub nxt: ThreadList,
    pub vc: VisitedTable,
    pub vn: VisitedTable,
    pub arena: SavesArena,
    pub step_id: u32,
}

impl Workspace {
    pub fn for_program(prog: &Program) -> Self {
        let n = prog.len();
        let stride = detect_stride(prog);
        Self {
            cur: ThreadList::with_capacity(n),
            nxt: ThreadList::with_capacity(n),
            vc: VisitedTable::with_size(n),
            vn: VisitedTable::with_size(n),
            arena: SavesArena::with_capacity_and_stride(n, stride),
            step_id: 0,
        }
    }

    pub fn next_step_id(&mut self) -> u32 {
        self.step_id += 1;
        self.step_id
    }
}

/// ASCII case-insensitive char compare. Port of `char_eq` —
/// matches C's behaviour exactly (only the basic Latin uppercase /
/// lowercase pair when `i` flag is set; no Unicode case-fold).
pub fn char_eq(a: u8, b: u8, flags: u8) -> bool {
    if a == b {
        return true;
    }
    if flags & RE_FLAG_I != 0 {
        if a.is_ascii_uppercase() && b == a + 32 {
            return true;
        }
        if a.is_ascii_lowercase() && b == a - 32 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::parser::Parser;
    use crate::program::Inst;

    fn build(pat: &str, flags: u8) -> Program {
        let mut p = Parser::new(pat.as_bytes(), flags);
        let root = p.parse().expect("parse failed");
        let mut prog = Program::new();
        compile(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        // Mirror `regex/compile.rs` — `Program::has_save` is the
        // compile-time cache of `insts.iter().any(...Op::Save)` and
        // production callers always set it; tests that build a
        // Program manually must set it too so `prog_has_save` reflects
        // truth (attack #J reads this field, not the live insts).
        prog.has_save = prog.any_save();
        prog
    }

    #[test]
    fn char_eq_case_sensitive_default() {
        assert!(char_eq(b'a', b'a', 0));
        assert!(!char_eq(b'a', b'A', 0));
    }

    #[test]
    fn char_eq_case_insensitive_under_i_flag() {
        assert!(char_eq(b'a', b'A', RE_FLAG_I));
        assert!(char_eq(b'A', b'a', RE_FLAG_I));
        // Non-letter bytes: no fold.
        assert!(!char_eq(b'0', b'1', RE_FLAG_I));
    }

    #[test]
    fn workspace_allocates_for_program_size() {
        let prog = build("a", 0);
        let ws = Workspace::for_program(&prog);
        assert_eq!(ws.cur.list.capacity(), prog.len());
        assert_eq!(ws.vc.visited.len(), prog.len());
    }

    #[test]
    fn search_literal_match_at_offset() {
        let prog = build("abc", 0);
        let r = search_from(&prog, b"xxabcyy", 0, 0, None).expect("hit");
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn search_literal_miss_returns_none() {
        let prog = build("abc", 0);
        assert!(search_from(&prog, b"xyz", 0, 0, None).is_none());
    }

    #[test]
    fn search_with_alternation() {
        let prog = build("cat|dog", 0);
        let r = search_from(&prog, b"the dog runs", 0, 0, None).expect("hit");
        assert_eq!(r.start, 4);
        assert_eq!(r.end, 7);
    }

    #[test]
    fn search_with_star_quantifier_greedy() {
        let prog = build("a*", 0);
        let r = search_from(&prog, b"aaab", 0, 0, None).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 3);
    }

    #[test]
    fn search_captures_group() {
        let prog = build("(\\d+)", 0);
        let r = search_from(&prog, b"x42y", 0, 0, None).expect("hit");
        assert_eq!(r.start, 1);
        assert_eq!(r.end, 3);
        assert_eq!(r.saves()[2], 1); // group 1 start
        assert_eq!(r.saves()[3], 3); // group 1 end
    }

    #[test]
    fn dfa_path_capture_group_extracts_saves_via_second_pass() {
        // chunk 9: a capture group like `(abc)` now passes the
        // `prog_ops_dfa_safe` gate. The DFA finds [start..end] and
        // the wire's second-pass Pike VM extracts captures. Test
        // `build` helper uses `compiler::compile` directly (no
        // implicit whole-match Save 0/1 wrap — that's added by
        // production regex/compile.rs), so we assert on group 1's
        // slots only, mirroring `search_captures_group` above.
        let prog = build("(abc)", 0);
        assert!(prog_has_save(&prog));
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        let r = search_from(&prog, b"xxabcyy", 0, 0, None).expect("hit");
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 5);
        assert_eq!(r.saves()[2], 2); // group 1 start
        assert_eq!(r.saves()[3], 5); // group 1 end
    }

    #[test]
    fn dfa_path_capture_alternation_extracts_saves() {
        // `(cat|dog)` — DFA finds 3-byte match, NFA picks leftmost
        // branch and writes its capture slots.
        let prog = build("(cat|dog)", 0);
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        let r = search_from(&prog, b"--dog--", 0, 0, None).expect("hit");
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 5);
        assert_eq!(r.saves()[2], 2);
        assert_eq!(r.saves()[3], 5);
    }

    #[test]
    fn dfa_path_capture_quantified_inner_extracts_saves() {
        // `(a+)b` — quantified inner capture. Group 1 covers the
        // run of `a`s. DFA scans for `a+b`; NFA second-pass commits
        // the longest `a+` run that lets `b` match.
        let prog = build("(a+)b", 0);
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        let r = search_from(&prog, b"  aaab xx", 0, 0, None).expect("hit");
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 6);
        assert_eq!(r.saves()[2], 2);
        assert_eq!(r.saves()[3], 5); // inner cap covers "aaa"
    }

    #[test]
    fn dfa_path_no_save_pattern_skips_second_pass() {
        // Patterns without any SAVE ops keep saves all-`-1` and
        // never invoke the second-pass NFA (perf-relevant — the
        // common `/literal/` case stays a pure DFA walk).
        let prog = build("abc", 0);
        assert!(!prog_has_save(&prog));
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        let r = search_from(&prog, b"xxabcyy", 0, 0, None).expect("hit");
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 5);
        // NoSaves answers an empty row; save_slot reads it as all -1.
        assert!(r.saves().is_empty());
        assert_eq!(save_slot(r.saves(), 0), -1);
        assert_eq!(save_slot(r.saves(), 1), -1);
    }

    #[test]
    fn match_anchor_only_at_specified_pos() {
        let prog = build("abc", 0);
        assert_eq!(match_anchor(&prog, b"xabc", 0, 0), None);
        let r = match_anchor(&prog, b"xabc", 1, 0).expect("hit");
        assert_eq!(r.start, 1);
        assert_eq!(r.end, 4);
    }

    #[test]
    fn case_insensitive_match() {
        let prog = build("Hello", RE_FLAG_I);
        let r = search_from(&prog, b"hello world", 0, RE_FLAG_I, None).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn anchor_beg_only_matches_at_start() {
        let prog = build("^abc", 0);
        assert!(search_from(&prog, b"xabc", 0, 0, None).is_none());
        assert!(search_from(&prog, b"abcx", 0, 0, None).is_some());
    }

    #[test]
    fn anchor_end_only_matches_at_end() {
        let prog = build("abc$", 0);
        assert!(search_from(&prog, b"abcx", 0, 0, None).is_none());
        assert!(search_from(&prog, b"xabc", 0, 0, None).is_some());
    }

    #[test]
    fn dfa_path_anchor_e_drives_at_end_accept() {
        // chunk 8.6a: `/foo$/` exercises the DFA's `is_accept_at_end`
        // path. Match commits only when the live state lands on the
        // haystack end with `Op::AnchorE` reachable via at-end ε.
        let prog = build("foo$", 0);
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        let r = search_from(&prog, b"xx foo", 0, 0, None).expect("hit");
        assert_eq!(r.start, 3);
        assert_eq!(r.end, 6);
        // Same `foo` not at end: no hit.
        assert!(search_from(&prog, b"foo bar", 0, 0, None).is_none());
        // Multiple `foo`s, only the trailing one matches.
        let r = search_from(&prog, b"foo foo", 0, 0, None).expect("hit");
        assert_eq!(r.start, 4);
        assert_eq!(r.end, 7);
    }

    #[test]
    fn dfa_path_anchor_e_only_pattern_matches_zero_width_end() {
        // `/$/` on any haystack matches the zero-width position at
        // `hay.len()`. With the DFA wire, `dfa_search` at offset 0
        // misses (no byte consumer), so the outer `search_from`
        // advances `st` until it hits `hay.len()` where the empty
        // suffix's at-end accept fires.
        let prog = build("$", 0);
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        let r = search_from(&prog, b"abc", 0, 0, None).expect("hit");
        assert_eq!(r.start, 3);
        assert_eq!(r.end, 3);
    }

    #[test]
    fn dfa_path_wbound_picks_correct_mid_entry() {
        // chunk 8.6b: `/\bfoo/` on "xfoo" must miss at offset 1
        // (left='x' word, right='f' word → no boundary). On " foo"
        // it hits at offset 1.
        let prog = build("\\bfoo", 0);
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        // Word-word boundary check: "xfoo" — wire selects mid_word at
        // st=1; mid_word's first step has left=Word, right='f' (word)
        // → no boundary → no match at offset 1.
        // The outer search_from also advances st but every later st
        // also has word-prev. So no hit anywhere.
        assert!(search_from(&prog, b"xfoo", 0, 0, None).is_none());
        // " foo": st=0 anchored start, left=None / non-word, right
        // =' ' (non-word) — boundary needs lw != rw; both false →
        // no boundary; NFA-equivalent dfa step finds none at st=0.
        // st=1 mid_nonword (prev=' '), left=NonWord, right='f' (word)
        // → boundary → match starting at st=1, end=4.
        let r = search_from(&prog, b" foo", 0, 0, None).expect("hit");
        assert_eq!(r.start, 1);
        assert_eq!(r.end, 4);
        // At text-start "foo": left=None / non-word, right='f' /
        // word → boundary → match at st=0.
        let r = search_from(&prog, b"foo", 0, 0, None).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 3);
    }

    #[test]
    fn dfa_path_nwbound_picks_correct_mid_entry() {
        // chunk 8.6b: `/\Bfoo/` — NWBound advances inside word runs.
        // "xfoo": st=0 (left=None / non-word, right='x' / word —
        // boundary, NWBound blocks). st=1 mid_word (left=Word), the
        // wire selects mid_word; first step right='f' / word → no
        // boundary → NWBound advances → match at st=1, end=4.
        let prog = build("\\Bfoo", 0);
        assert!(crate::dfa::prog_ops_dfa_safe(&prog));
        let r = search_from(&prog, b"xfoo", 0, 0, None).expect("hit");
        assert_eq!(r.start, 1);
        assert_eq!(r.end, 4);
        // Standalone "foo": every position has a boundary (text-
        // start to 'f', or end of "foo"), so NWBound finds nothing.
        assert!(search_from(&prog, b"foo", 0, 0, None).is_none());
    }

    // Round 3 Phase B sub-batch 4 attack #R-J v4 — regex-024 fix
    // diagnostic. Both subcases below are reproductions of the two
    // failing fixtures in `conformance/cases/regex-024-uflag-unsafe-
    // class-capture.ts`: multi-Op concat patterns mixing K-PROPERTY
    // with capture groups that v3 mis-built.
    #[test]
    fn path_a_v4_regex024_subcase_1_letter_digit_letter_capture() {
        // /(\p{L})(\d+)(\p{L})/u on "x123Ω".
        // Expected m[1..3] = "x", "123", "Ω"; total bytes = 1 + 3 + 2 = 6.
        let flags = crate::parser::RE_FLAG_U;
        let prog = build("(\\p{L})(\\d+)(\\p{L})", flags);
        let hay = "x123\u{03A9}".as_bytes();
        // Sanity: direct DFA search must also see the match (v3-A
        // regression returned None here because the post-`\d+`-loop
        // state had a multi-PC ready set containing both K-PROPERTY
        // and non-K-PROPERTY byte-consumers; v3-A's
        // `classify_kproperty_shape` rejected such states and the
        // chunk-10d byte_step ASCII-only path then dropped Ω).
        let dfa = crate::dfa::build_dfa(&prog, flags);
        assert_eq!(
            crate::dfa::dfa_search(&dfa, &prog, hay),
            Some(6),
            "v4 — direct DFA must find /(\\p{{L}})(\\d+)(\\p{{L}})/u over \
             x123Ω as 6 bytes (was None under v3-A)"
        );
        let r = search_from(&prog, hay, 0, flags, None).expect("subcase 1 must match");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 6);
        // Captures via Pike VM second-pass.
        assert_eq!(&hay[r.saves()[2] as usize..r.saves()[3] as usize], b"x");
        assert_eq!(&hay[r.saves()[4] as usize..r.saves()[5] as usize], b"123");
        assert_eq!(
            &hay[r.saves()[6] as usize..r.saves()[7] as usize],
            "\u{03A9}".as_bytes(),
        );
    }

    #[test]
    fn path_a_v4_regex024_subcase_2_letter_nonletter_letter_capture() {
        // /(\p{L}+)(\P{L}+)(\p{L}+)/u on "abc 漢字" — capture:
        // ("abc"," ","漢字"). Multi-K-PROPERTY chain mixed with K-NEG
        // (`\P{L}` negates → utf8_class_expand emits byte-only Class
        // chain). v3-A's post-第一-`\p{L}+` ready set is mixed
        // K-PROPERTY + byte_only Class which the v3 gate rejected.
        // v4 keeps transitions[] for the byte_only path AND pending_
        // class fallback for the K-PROPERTY path.
        let flags = crate::parser::RE_FLAG_U;
        let prog = build("(\\p{L}+)(\\P{L}+)(\\p{L}+)", flags);
        let hay = "abc \u{6F22}\u{5B57}";
        // 3 (abc) + 1 (space) + 6 (2*3 CJK bytes) = 10
        let dfa = crate::dfa::build_dfa(&prog, flags);
        assert_eq!(
            crate::dfa::dfa_search(&dfa, &prog, hay.as_bytes()),
            Some(10),
            "v4 — direct DFA must find /(\\p{{L}}+)(\\P{{L}}+)(\\p{{L}}+)/u over \
             'abc 漢字' as 10 bytes",
        );
        let r = search_from(&prog, hay.as_bytes(), 0, flags, None).expect("subcase 2 must match");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 10);
        assert_eq!(
            &hay.as_bytes()[r.saves()[2] as usize..r.saves()[3] as usize],
            b"abc"
        );
        assert_eq!(
            &hay.as_bytes()[r.saves()[4] as usize..r.saves()[5] as usize],
            b" "
        );
        assert_eq!(
            &hay.as_bytes()[r.saves()[6] as usize..r.saves()[7] as usize],
            "\u{6F22}\u{5B57}".as_bytes(),
        );
    }
}
