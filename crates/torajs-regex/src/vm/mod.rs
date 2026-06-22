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
    /// `REGEX_SAVE_SLOTS` `i64`s holding this thread's capture saves.
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

/// Pool of capture-save rows. Each `alloc_*` returns a `u32` handle
/// indexing into a flat `Vec<i64>` of `stride`-wide rows.
///
/// V0.2 P14-S12 introduced the arena (Thread shrinks 528 → 24 bytes).
/// V0.2 P14-S13 makes `stride` per-Program: `2 * (n_captures + 1)`
/// (slots 0/1 hold the whole-match start/end; slots `2*i` / `2*i+1`
/// hold capture group `i`'s start/end). For the common case of zero
/// or one user capture group, stride is 2 or 4 — vs. the pre-S13
/// fixed `REGEX_SAVE_SLOTS = 64`, that shrinks each `Op::Save`
/// `alloc_clone` row copy from 512 bytes to 16-32 bytes.
#[derive(Debug)]
pub struct SavesArena {
    pub data: Vec<i64>,
    /// Width of each row in slots (`i64` count). Always
    /// `<= REGEX_SAVE_SLOTS`. Caller `out_saves` buffer stays a
    /// fixed `[i64; REGEX_SAVE_SLOTS]` so high-slot reads against a
    /// match without that many captures return the caller's `-1`
    /// init sentinel — `vm_match_at`'s writeback only fills
    /// `arena.get(id)[..stride]`.
    pub stride: usize,
}

impl SavesArena {
    pub fn with_capacity_and_stride(rows: usize, stride: usize) -> Self {
        Self {
            data: Vec::with_capacity(rows * stride),
            stride,
        }
    }

    pub fn reset(&mut self) {
        self.data.clear();
    }

    /// Allocate a fresh row initialised to `-1` (sentinel for
    /// "not captured"). Returns the row's handle.
    pub fn alloc_empty(&mut self) -> u32 {
        let stride = self.stride;
        let id = (self.data.len() / stride) as u32;
        let new_len = self.data.len() + stride;
        self.data.resize(new_len, -1);
        id
    }

    /// Allocate a fresh row pre-populated with a copy of `src_id`'s
    /// row. Used by `Op::Save` to fork a per-branch saves snapshot.
    pub fn alloc_clone(&mut self, src_id: u32) -> u32 {
        let stride = self.stride;
        let src_start = (src_id as usize) * stride;
        let new_start = self.data.len();
        let new_id = (new_start / stride) as u32;
        self.data.resize(new_start + stride, -1);
        self.data
            .copy_within(src_start..src_start + stride, new_start);
        new_id
    }

    /// Read-only access to a row.
    pub fn get(&self, id: u32) -> &[i64] {
        let stride = self.stride;
        let start = (id as usize) * stride;
        &self.data[start..start + stride]
    }

    /// Write a single slot.
    pub fn write_slot(&mut self, id: u32, slot: usize, val: i64) {
        let start = (id as usize) * self.stride;
        self.data[start + slot] = val;
    }
}

/// Scan a Program for the highest `OP_SAVE` slot reference and
/// derive the per-row stride of the saves arena (slot 0/1 = whole
/// match, slot `2*i`/`2*i+1` = capture group `i`). Returns the row
/// width in `i64` slots. Programs without any `OP_SAVE` get a stride
/// of `2` (enough to hold the implicit whole-match slots if SSA-lower
/// later adds them); the true minimum useful stride is 0 but a
/// non-zero stride keeps `alloc_*` from degenerating into no-op rows
/// for the never-takes-this-branch case.
fn detect_stride(prog: &Program) -> usize {
    let mut max_slot: i32 = -1;
    for inst in &prog.insts {
        if inst.op == crate::program::Op::Save as u8 && inst.a > max_slot {
            max_slot = inst.a;
        }
    }
    if max_slot < 0 {
        2
    } else {
        (max_slot as usize) + 1
    }
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
    let mut st = from_pos;
    // V0.2 P14 — DFA fast path (chunk 7 v3, per-call build).
    // `build_dfa(prog)` runs once per call instead of caching on
    // `Program`. The chunk 6 lazy cache shipped first but both the
    // `UnsafeCell` and the chunk-7.5 `OnceCell` variants triggered a
    // flaky SIGBUS in `regex-021-test-lastindex` once consumed from the
    // hot path; the cache was deleted as dead substrate pending a
    // chunk 7.6 deep audit of the RegExp/Program lifetime vs cached
    // `&DfaProgram` across the SSA-lower ABI boundary.
    //
    // Flag gate:
    // - `Program::can_dfa` excludes backref + lookaround.
    // - `dfa::prog_ops_dfa_safe` excludes SAVE / AnchorE / WBound /
    //   NWBound. (chunk 8.5 lifted AnchorB from this list — the DFA
    //   now resolves `^` via `start` / `start_mid` start states.)
    // - `flags & RE_FLAG_U == 0` — DFA byte-step lacks code-point
    //   awareness (`u` flag needs UTF-8 decode at boundaries — future
    //   chunk 10).
    // - AnyChar requires `s` flag (DFA always advances on `.`).
    //
    // chunk 8.7: `RE_FLAG_I` no longer a blocker — `build_dfa` threads
    // `flags` through `byte_step`, which calls `char_eq` /
    // `class_test_case_fold` for ASCII case-pair resolution.
    // chunk 8.8: `RE_FLAG_M` no longer a blocker — `build_dfa` threads
    // `mflag` into `PositionCtx`, so `Op::AnchorB` advances when the
    // ctx is at text-start *or* `left_byte == Some(b'\n')` (line-start
    // under multiline).
    let flag_blockers = crate::parser::RE_FLAG_U;
    let dfa_fast_path = prog.can_dfa
        && (flags & flag_blockers) == 0
        && crate::dfa::prog_ops_dfa_safe(prog)
        && (!crate::dfa::prog_uses_anychar(prog) || (flags & crate::parser::RE_FLAG_S) != 0);
    let dfa_built = if dfa_fast_path {
        Some(crate::dfa::build_dfa(prog, flags))
    } else {
        None
    };
    loop {
        // V0.2 P14-S2 — literal-prefix SIMD anchor. When the
        // compiled program's leading byte-consuming op is a plain
        // `Char(b)` (compile.rs detects this), any candidate start
        // position whose first input byte is not `b` can never
        // match — memchr-skip past the gap. The Pike VM scan on
        // `"abbc xxx abc yyy abbbbc"` for `/zzz/g` drops from 24
        // `vm_match_at` calls (~1000 ns) to a single memchr-miss
        // returning None (~5 ns). `prefix_byte` is None for
        // patterns whose first op is `AnyChar` / `Class` / `Split`
        // / `Backref` / lookaround, or when the i flag is set
        // (case-insensitive defeats single-byte memchr) — those
        // fall through to the original per-position simulation.
        if let Some(b) = prog.prefix_byte {
            if st >= slen {
                // A program with a literal-prefix anchor must
                // consume at least one byte to match — no chance
                // of a zero-width match at end-of-string.
                return None;
            }
            let hay = &s[st as usize..];
            match hay.iter().position(|&c| c == b) {
                Some(off) => st += off as i64,
                None => return None,
            }
        } else if st > slen {
            return None;
        }
        // Under u flag, start positions must land on code-point
        // boundaries — skip UTF-8 continuation bytes so the matcher
        // doesn't decode mid-sequence and accidentally satisfy
        // `[^\p{...}]`. P9.3-A2.
        if flags & crate::parser::RE_FLAG_U != 0 && st < slen && s[st as usize] & 0xC0 == 0x80 {
            st += 1;
            continue;
        }
        if let Some(dfa) = dfa_built.as_ref() {
            // Anchored DFA at byte offset `st`. The DFA is byte-step
            // only — capture slots stay all -1 (gate guarantees no SAVE
            // ops, so the Pike VM would emit the same all-`-1` saves).
            //
            // chunk 8.5 / 8.8 entry-state selection. Use the
            // text-start closure (`dfa.start` — `^` advanced) when the
            // cursor is at a line boundary: byte 0 of the haystack, or
            // — under `RE_FLAG_M` — immediately after a `\n` byte.
            // Otherwise use `dfa.start_mid` (`^` blocked). Patterns
            // without `Op::AnchorB` dedup the two indices, so the
            // branch is free.
            let at_line_start = st == 0
                || (flags & crate::parser::RE_FLAG_M != 0
                    && st > 0
                    && s[(st - 1) as usize] == b'\n');
            let hay_suffix = &s[st as usize..];
            let hit = if at_line_start {
                crate::dfa::dfa_search(dfa, hay_suffix)
            } else {
                crate::dfa::dfa_search_mid(dfa, hay_suffix)
            };
            if let Some(n) = hit {
                return Some(MatchResult {
                    start: st,
                    end: st + n as i64,
                    saves: [-1i64; REGEX_SAVE_SLOTS],
                });
            }
            // Anchored DFA missed at `st`; advance one byte and retry.
            st += 1;
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
        st += 1;
    }
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
