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
//!   [`Workspace`] data structures + [`char_eq`] + public entry
//!   points [`search_from`] / [`search_from_with_ws`] /
//!   [`match_anchor`].
//! - [`dispatch`] — `add_thread` (epsilon expansion) + `sub_probe`
//!   (lookahead) + `sub_probe_ending_at` (lookbehind).
//! - [`match_at`] — `vm_match_at` inner loop (operand dispatch on
//!   the input-consuming op set).

pub mod dispatch;
pub mod match_at;

use crate::node::REGEX_SAVE_SLOTS;
use crate::parser::RE_FLAG_I;
use crate::program::Program;

/// Per-thread state in the Pike NFA. Each step the matcher iterates
/// every Thread in `cur` and advances PCs to `nxt` (or, for backref
/// continuation / u-flag deferred bytes, back to `nxt` at the same
/// `pc` with mutated bookkeeping).
#[derive(Clone, Debug)]
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
    /// Capture-group save slots, indexed `2*idx` (start) / `2*idx+1`
    /// (end). `-1` sentinel = "not captured". Cloned across SPLIT
    /// forks so a SAVE in one branch doesn't leak into the other.
    pub saves: [i64; REGEX_SAVE_SLOTS],
}

impl Thread {
    pub fn empty() -> Self {
        Self {
            pc: 0,
            br_offset: 0,
            u_skip: 0,
            saves: [-1; REGEX_SAVE_SLOTS],
        }
    }
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

/// Reusable per-Program workspace — allocates once at search start;
/// re-used across tight-loop iterations (replaceAll / matchAll /
/// split) via [`search_from_with_ws`]. Size: `2 * (n_insts *
/// sizeof::<Thread>() + n_insts * 4)` ≈ `2 * n_insts * 540 B`. For
/// a 100-instruction program, ≈ 108 KB per workspace.
#[derive(Debug)]
pub struct Workspace {
    pub cur: ThreadList,
    pub nxt: ThreadList,
    pub vc: VisitedTable,
    pub vn: VisitedTable,
    pub step_id: u32,
}

impl Workspace {
    pub fn for_program(prog: &Program) -> Self {
        let n = prog.len();
        Self {
            cur: ThreadList::with_capacity(n),
            nxt: ThreadList::with_capacity(n),
            vc: VisitedTable::with_size(n),
            vn: VisitedTable::with_size(n),
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

/// Successful match outcome from [`search_from`] / [`match_anchor`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchResult {
    pub start: i64,
    pub end: i64,
    /// Capture-group save slots (size [`REGEX_SAVE_SLOTS`]); slot
    /// `2*idx` = group `idx` start, `2*idx + 1` = group `idx` end.
    /// `-1` sentinel = "not captured".
    pub saves: [i64; REGEX_SAVE_SLOTS],
}

/// Search for a match starting at any position `>= from_pos`. Returns
/// `Some(MatchResult)` on hit, `None` on miss. Allocates a fresh
/// [`Workspace`] internally — for tight loops use
/// [`search_from_with_ws`].
pub fn search_from(prog: &Program, s: &[u8], from_pos: i64, flags: u8) -> Option<MatchResult> {
    if prog.is_empty() {
        return None;
    }
    let mut ws = Workspace::for_program(prog);
    search_from_with_ws(prog, s, from_pos, flags, &mut ws)
}

/// Tight-loop variant of [`search_from`]: caller owns the workspace
/// so per-iter alloc is skipped. `Workspace::step_id` is shared so
/// visited bitmaps stay coherent across find calls on the same
/// workspace.
pub fn search_from_with_ws(
    prog: &Program,
    s: &[u8],
    from_pos: i64,
    flags: u8,
    ws: &mut Workspace,
) -> Option<MatchResult> {
    let slen = s.len() as i64;
    for st in from_pos..=slen {
        // Under u flag, start positions must land on code-point
        // boundaries — skip UTF-8 continuation bytes so the matcher
        // doesn't decode mid-sequence and accidentally satisfy
        // `[^\p{...}]`. P9.3-A2.
        if flags & crate::parser::RE_FLAG_U != 0 && st < slen && s[st as usize] & 0xC0 == 0x80 {
            continue;
        }
        let mut saves = [-1i64; REGEX_SAVE_SLOTS];
        let end = match_at::vm_match_at(prog, s, st, flags, ws, Some(&mut saves), -1);
        if end >= 0 {
            return Some(MatchResult {
                start: st,
                end,
                saves,
            });
        }
    }
    None
}

/// P9.4 — anchored single-position match for the sticky (`y`) flag.
/// Tries exactly `at` and reports hit/miss. Under u flag, an `at`
/// landing on a UTF-8 continuation byte is a miss.
pub fn match_anchor(prog: &Program, s: &[u8], at: i64, flags: u8) -> Option<MatchResult> {
    if prog.is_empty() {
        return None;
    }
    let slen = s.len() as i64;
    if at < 0 || at > slen {
        return None;
    }
    if flags & crate::parser::RE_FLAG_U != 0 && at < slen && s[at as usize] & 0xC0 == 0x80 {
        return None;
    }
    let mut ws = Workspace::for_program(prog);
    let mut saves = [-1i64; REGEX_SAVE_SLOTS];
    let end = match_at::vm_match_at(prog, s, at, flags, &mut ws, Some(&mut saves), -1);
    if end >= 0 {
        Some(MatchResult {
            start: at,
            end,
            saves,
        })
    } else {
        None
    }
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
        compile(&mut prog, &root);
        prog.emit(Inst::match_accept());
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
        let r = search_from(&prog, b"xxabcyy", 0, 0).expect("hit");
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn search_literal_miss_returns_none() {
        let prog = build("abc", 0);
        assert!(search_from(&prog, b"xyz", 0, 0).is_none());
    }

    #[test]
    fn search_with_alternation() {
        let prog = build("cat|dog", 0);
        let r = search_from(&prog, b"the dog runs", 0, 0).expect("hit");
        assert_eq!(r.start, 4);
        assert_eq!(r.end, 7);
    }

    #[test]
    fn search_with_star_quantifier_greedy() {
        let prog = build("a*", 0);
        let r = search_from(&prog, b"aaab", 0, 0).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 3);
    }

    #[test]
    fn search_captures_group() {
        let prog = build("(\\d+)", 0);
        let r = search_from(&prog, b"x42y", 0, 0).expect("hit");
        assert_eq!(r.start, 1);
        assert_eq!(r.end, 3);
        assert_eq!(r.saves[2], 1); // group 1 start
        assert_eq!(r.saves[3], 3); // group 1 end
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
        let r = search_from(&prog, b"hello world", 0, RE_FLAG_I).expect("hit");
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn anchor_beg_only_matches_at_start() {
        let prog = build("^abc", 0);
        assert!(search_from(&prog, b"xabc", 0, 0).is_none());
        assert!(search_from(&prog, b"abcx", 0, 0).is_some());
    }

    #[test]
    fn anchor_end_only_matches_at_end() {
        let prog = build("abc$", 0);
        assert!(search_from(&prog, b"abcx", 0, 0).is_none());
        assert!(search_from(&prog, b"xabc", 0, 0).is_some());
    }
}
