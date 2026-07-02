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
pub mod result;
pub mod saves_arena;

pub use result::{EMPTY_SAVES, MatchResult};
pub use saves_arena::SavesArena;
use saves_arena::detect_stride;

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

/// Search for a match starting at any position `>= from_pos`. Returns
/// `Some(MatchResult)` on hit, `None` on miss. Allocates a fresh
/// [`Workspace`] internally — for tight loops use
/// [`search_from_with_ws`].
///
/// `dfa_cached`(Round 3 Phase B sub-batch 7.2)— DFA borrowed once at
/// the caller side. Every production surface caller in
/// `crate::regex::*` (replace / replace_fn / match_op / test_find /
/// match_all / split) now passes
/// `re.baked_dfa_view().as_ref().or(re.dfa_runtime.as_ref())` — the
/// AOT-baked view for AOT-eligible literal regexes, else the runtime
/// DFA eager-built at `__torajs_regex_compile` ctor and owned by
/// `RegExp.dfa_runtime`. `None` is left as a vm-test / internal-
/// fallback path: when the caller didn't (or couldn't) bind a DFA,
/// the function inline-builds `build_dfa(prog, flags)` per call so
/// vm unit tests with `dfa_cached = None` still exercise the DFA
/// fast path. The dead-on-the-surface fallback is preserved
/// intentionally — vm tests rely on it for DFA-path regression
/// coverage (e.g. `path_a_v4_regex024_subcase_*`).
pub fn search_from(
    prog: &Program,
    s: &[u8],
    from_pos: i64,
    flags: u8,
    dfa_cached: Option<&crate::dfa::DfaProgram>,
) -> Option<MatchResult> {
    if prog.is_empty() {
        return None;
    }
    let mut ws = Workspace::for_program(prog);
    // Round 3 Phase B attack #R-A1: callers without ASCII-pre-classification
    // (tests, internal entry points) default `haystack_is_ascii = false` —
    // safe (the u-flag continuation-byte gate still fires as before).
    // Hot-loop callers route through `search_from_with_ws` and pass `true`
    // when they've already established the haystack is ASCII via
    // `str_slice_ascii_view`.
    search_from_with_ws(prog, s, from_pos, flags, &mut ws, dfa_cached, false, true)
}

/// Tight-loop variant of [`search_from`]: caller owns the workspace
/// so per-iter alloc is skipped. `Workspace::step_id` is shared so
/// visited bitmaps stay coherent across find calls on the same
/// workspace.
///
/// `haystack_is_ascii` (Round 3 Phase B attack #R-A1) — caller asserts
/// the haystack `s` contains no UTF-8 continuation bytes (`0x80..=0xBF`).
/// When `true`, the u-flag continuation-byte skip in the outer loop
/// short-circuits, saving ~12 ns/iter on ASCII-only haystacks under
/// `RE_FLAG_U`. `false` keeps the original per-iter check (safe for
/// non-ASCII / unknown haystacks).
///
/// `want_saves` (Round 5 attack str-replace #1) — caller asserts it
/// will actually read `MatchResult::saves()`. When `false` on a
/// program WITH `Op::Save`, the DFA hit returns `no_saves` directly,
/// skipping the 512-byte `[i64; 64]` init AND the second-pass Pike VM
/// capture extraction (~150 ns/hit) — the JSC-parity dollarless
/// `replace` fast path. Callers that consume captures (exec /
/// matchAll / split / replace-with-`$` / fn-replace) pass `true`.
#[allow(clippy::too_many_arguments)]
pub fn search_from_with_ws(
    prog: &Program,
    s: &[u8],
    from_pos: i64,
    flags: u8,
    ws: &mut Workspace,
    dfa_cached: Option<&crate::dfa::DfaProgram>,
    haystack_is_ascii: bool,
    want_saves: bool,
) -> Option<MatchResult> {
    let slen = s.len() as i64;
    let mut st = from_pos;
    // V0.2 P14 — DFA fast path.
    // History: chunk 6 lazy `UnsafeCell<Option<DfaProgram>>` cache and
    // the chunk-7.5 `OnceCell` variant both triggered a flaky SIGBUS
    // in `regex-021-test-lastindex` from the hot path; chunk 7 v3
    // deleted that interior-mutable cache and moved DFA build into
    // this function (per-call). Round 3 Phase B sub-batch 7.2
    // (2026-06-25) moved the build back out — `RegExp.dfa_runtime`
    // eager-builds once at `__torajs_regex_compile` ctor with a plain
    // `Option<DfaProgram>` (no `UnsafeCell`), so the chunk-7.6 SIGBUS
    // UB family is structurally closed without losing per-RegExp
    // amortisation. Surface callers now always pass `dfa_cached =
    // Some(...)` for DFA-eligible programs; the `dfa_built_local`
    // fallback below stays for vm-tests / internal callers without
    // a RegExp host (`fn search_from` reaches us with
    // `dfa_cached = None`).
    //
    // Flag gate:
    // - `Program::can_dfa` excludes backref + lookaround.
    // - `dfa::prog_ops_dfa_safe` no longer rejects any opcode — chunks
    //   8.5 / 8.6a / 8.6b / 8.7 / 8.8 / 9 / 10a cleared `^` / `$` / `\b`
    //   / `\B` / RE_FLAG_I (i) / RE_FLAG_M (m) / SAVE / AnyChar-w/o-s;
    //   the function stays as a safety net for future opcode adds.
    // - chunk 10d cleared the last u-flag blocker — unsafe classes
    //   (negate / `u_props` / non-ASCII bits) get compile-time
    //   rewritten by `utf8_class_expand` into a byte-level Alt-of-
    //   Concat over `Op::Class` instructions referencing
    //   `byte_only` leaf classes (re2 / regex-syntax `Utf8Sequences`
    //   range-based byte expansion), which the DFA byte-step walks
    //   verbatim. The Pike VM second-pass for capture extraction
    //   sees the same instructions and honours `byte_only` in
    //   `match_at.rs::Op::Class`, so a `\p{L}u` pattern with a
    //   capture group is fully DFA-resident.
    //
    // On hit, when the program emits any `Op::Save`, the wire below
    // runs `vm_match_at(.., end_target = st + n)` for a second pass
    // that produces the winning thread's `saves`.
    let dfa_fast_path = prog.can_dfa && crate::dfa::prog_ops_dfa_safe(prog);
    // Round 3 Phase B sub-batch 7.2 (2026-06-25) — production surface
    // callers always provide a `dfa_cached` (AOT-baked view or
    // `RegExp.dfa_runtime`). The local build fires only for vm tests /
    // internal entry points without a RegExp host (e.g. `search_from`
    // which forwards `dfa_cached = None`); preserved for DFA-path
    // regression coverage in unit tests (`path_a_v4_regex024_subcase_*`
    // and friends explicitly walk `dfa_cached = None` to exercise the
    // build path on toy programs).
    let dfa_built_local = if dfa_fast_path && dfa_cached.is_none() {
        Some(crate::dfa::build_dfa(prog, flags))
    } else {
        None
    };
    let dfa_built: Option<&crate::dfa::DfaProgram> = if dfa_fast_path {
        dfa_cached.or(dfa_built_local.as_ref())
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
        //
        // Round 3 Phase B attack #R-A1: when the caller pre-classified
        // the haystack as ASCII (`haystack_is_ascii == true`), no byte
        // in `s` can have `& 0xC0 == 0x80`, so the per-iter check is
        // wasted work. Short-circuit to save ~12 ns/iter on ASCII
        // u-flag fixtures (the common case for `/\p{L}+/u` style
        // patterns against Latin-1 / ASCII inputs).
        if !haystack_is_ascii
            && flags & crate::parser::RE_FLAG_U != 0
            && st < slen
            && s[st as usize] & 0xC0 == 0x80
        {
            st += 1;
            continue;
        }
        if let Some(dfa) = dfa_built {
            // Anchored DFA at byte offset `st`. The DFA itself is
            // capture-blind (chunk 9: `Op::Save` is a no-op ε in the
            // closure), so the byte-step traversal finds `[st..end]`
            // without populating saves. When the pattern emitted any
            // `Op::Save` ops we run a second-pass Pike VM at the same
            // start with `end_target = end` — its leftmost-first
            // semantics under that length restriction match the DFA's
            // leftmost-longest hit, and the winning thread's snapshot
            // is the JS-spec capture set. Patterns with no SAVE ops
            // skip the second pass — saves stay all-`-1`.
            //
            // chunk 8.5 / 8.8 entry-state selection. Use the
            // text-start closure (`dfa.start` — `^` advanced) when the
            // cursor is at a line boundary: byte 0 of the haystack, or
            // — under `RE_FLAG_M` — immediately after a `\n` byte.
            // Otherwise use `dfa.start_mid` (`^` blocked). Patterns
            // without `Op::AnchorB` dedup the two indices, so the
            // branch is free.
            let hay_suffix = &s[st as usize..];
            // Round 3 Phase B attack #R-A2 — `all_starts_equal` is set
            // by `build_dfa` (and `baked_dfa_view`) when the four
            // anchored start indices collapse to one — patterns
            // without `^` / `\b` / `\B` / multiline-`^`. In that case
            // the `at_line_start` + `prev_is_word` selection is wasted
            // work; jump straight into `dfa_search` at `dfa.start`.
            // For `/\p{L}+/u`-style fixtures this saves ~12 ns/iter.
            let hit = if dfa.all_starts_equal {
                crate::dfa::dfa_search(dfa, prog, hay_suffix)
            } else {
                // chunk 8.6b — when not at a line-start, pick the mid
                // entry whose `LeftByteAttr` matches `s[st-1]`'s class
                // so `Op::WBound` on the first step sees the correct
                // left-byte class. Word class = ASCII `[A-Za-z0-9_]`,
                // mirroring `at_word_boundary`. Patterns without `\b`
                // / `\B` dedup the two mid states down.
                let at_line_start = st == 0
                    || (flags & crate::parser::RE_FLAG_M != 0
                        && st > 0
                        && s[(st - 1) as usize] == b'\n');
                if at_line_start {
                    crate::dfa::dfa_search(dfa, prog, hay_suffix)
                } else {
                    let prev = s[(st - 1) as usize];
                    let prev_is_word = prev.is_ascii_alphanumeric() || prev == b'_';
                    if prev_is_word {
                        crate::dfa::dfa_search_mid_word(dfa, prog, hay_suffix)
                    } else {
                        crate::dfa::dfa_search_mid_nonword(dfa, prog, hay_suffix)
                    }
                }
            };
            if let Some(n) = hit {
                let end = st + n as i64;
                // Round 3 Phase B sub-batch 5 attack #R-A3 — split the
                // no-saves fast path. `prog.has_save == false` skips
                // the per-iter `[-1i64; REGEX_SAVE_SLOTS]` stack init
                // entirely; the `EMPTY_SAVES` const ref is materialised
                // only when a caller actually calls `m.saves()`.
                if !prog_has_save(prog) || !want_saves {
                    return Some(MatchResult::no_saves(st, end));
                }
                let mut saves = [-1i64; REGEX_SAVE_SLOTS];
                // Second-pass NFA at exactly the DFA-found window to
                // pull captures out. If the NFA disagrees with the DFA
                // boundary we leave saves all-`-1` rather than report
                // a phantom — this should not happen since the DFA's
                // accept set is derived from the same NFA's `Op::Match`
                // PCs.
                let nfa_end = match_at::vm_match_at(prog, s, st, flags, ws, Some(&mut saves), end);
                if nfa_end != end {
                    saves = [-1i64; REGEX_SAVE_SLOTS];
                }
                return Some(MatchResult::with_saves(st, end, saves));
            }
            // Anchored DFA missed at `st`; advance one byte and retry.
            st += 1;
            continue;
        }
        // Pike-VM-only path (DFA gate refused). When the program has
        // no `Op::Save` ops we still need the second pass for the
        // `end` boundary (no DFA to give us one), but skip the slot
        // array entirely — `MatchResult::no_saves` carries the
        // `EMPTY_SAVES` sentinel.
        if !prog_has_save(prog) || !want_saves {
            let end = match_at::vm_match_at(prog, s, st, flags, ws, None, -1);
            if end >= 0 {
                return Some(MatchResult::no_saves(st, end));
            }
        } else {
            let mut saves = [-1i64; REGEX_SAVE_SLOTS];
            let end = match_at::vm_match_at(prog, s, st, flags, ws, Some(&mut saves), -1);
            if end >= 0 {
                return Some(MatchResult::with_saves(st, end, saves));
            }
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
    // Round 3 Phase B sub-batch 5 attack #R-A3 — skip the saves init
    // when the program has no `Op::Save`.
    if !prog_has_save(prog) {
        let end = match_at::vm_match_at(prog, s, at, flags, &mut ws, None, -1);
        if end >= 0 {
            Some(MatchResult::no_saves(at, end))
        } else {
            None
        }
    } else {
        let mut saves = [-1i64; REGEX_SAVE_SLOTS];
        let end = match_at::vm_match_at(prog, s, at, flags, &mut ws, Some(&mut saves), -1);
        if end >= 0 {
            Some(MatchResult::with_saves(at, end, saves))
        } else {
            None
        }
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
        compile(&mut prog, &root, flags);
        prog.emit(Inst::match_accept());
        // Mirror `regex/compile.rs` — `Program::has_save` is the
        // compile-time cache of `insts.iter().any(...Op::Save)` and
        // production callers always set it; tests that build a
        // Program manually must set it too so `prog_has_save` reflects
        // truth (attack #J reads this field, not the live insts).
        prog.has_save = prog
            .insts
            .iter()
            .any(|ins| ins.op == crate::program::Op::Save as u8);
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
        assert_eq!(r.saves()[0], -1);
        assert_eq!(r.saves()[1], -1);
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
