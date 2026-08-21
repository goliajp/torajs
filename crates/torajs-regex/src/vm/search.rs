//! Public search entry points — [`search_from`] / [`search_from_with_ws`]
//! / [`match_anchor`] — split out of `vm/mod.rs` (chunk 472; the ≤ 500
//! LOC HARD RULE). The outer scan loop wires three engines together:
//! the literal-prefix memchr anchor, the anchored-DFA fast path with a
//! Pike-VM second pass for captures, and the Pike-VM-only fallback for
//! programs the DFA gate refuses (backref / lookaround).

use super::{Workspace, match_at, prog_has_save};
use crate::program::Program;
use crate::vm::MatchResult;
use alloc::vec;

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
    let mut ws: Option<Workspace> = None;
    // Round 3 Phase B attack #R-A1: callers without ASCII-pre-classification
    // (tests, internal entry points) default `haystack_is_ascii = false` —
    // safe (the u-flag continuation-byte gate still fires as before).
    // Hot-loop callers route through `search_from_with_ws` and pass `true`
    // when they've already established the haystack is ASCII via
    // `str_slice_ascii_view`.
    search_from_with_ws(prog, s, from_pos, flags, &mut ws, dfa_cached, false, true)
}

/// Resolve the DFA the outer loop will walk, if any.
///
/// V0.2 P14 — DFA fast path.
/// History: chunk 6 lazy `UnsafeCell<Option<DfaProgram>>` cache and
/// the chunk-7.5 `OnceCell` variant both triggered a flaky SIGBUS
/// in `regex-021-test-lastindex` from the hot path; chunk 7 v3
/// deleted that interior-mutable cache and moved DFA build into
/// the search wire (per-call). Round 3 Phase B sub-batch 7.2
/// (2026-06-25) moved the build back out — `RegExp.dfa_runtime`
/// eager-builds once at `__torajs_regex_compile` ctor with a plain
/// `Option<DfaProgram>` (no `UnsafeCell`), so the chunk-7.6 SIGBUS
/// UB family is structurally closed without losing per-RegExp
/// amortisation. Surface callers now always pass `dfa_cached =
/// Some(...)` for DFA-eligible programs; the `local` build below
/// fires only for vm-tests / internal callers without a RegExp host
/// (`fn search_from` reaches us with `dfa_cached = None`) — preserved
/// for DFA-path regression coverage in unit tests
/// (`path_a_v4_regex024_subcase_*` and friends explicitly walk
/// `dfa_cached = None` to exercise the build path on toy programs).
///
/// Flag gate:
/// - `Program::can_dfa` excludes backref + lookaround.
/// - `dfa::prog_ops_dfa_safe` no longer rejects any opcode — chunks
///   8.5 / 8.6a / 8.6b / 8.7 / 8.8 / 9 / 10a cleared `^` / `$` / `\b`
///   / `\B` / RE_FLAG_I (i) / RE_FLAG_M (m) / SAVE / AnyChar-w/o-s;
///   the function stays as a safety net for future opcode adds.
/// - chunk 10d cleared the last u-flag blocker — unsafe classes
///   (negate / `u_prop_tables` / non-ASCII bits) get compile-time
///   rewritten by `utf8_class_expand` into a byte-level Alt-of-
///   Concat over `Op::Class` instructions referencing
///   `byte_only` leaf classes (re2 / regex-syntax `Utf8Sequences`
///   range-based byte expansion), which the DFA byte-step walks
///   verbatim. The Pike VM second-pass for capture extraction
///   sees the same instructions and honours `byte_only` in
///   `match_at.rs::Op::Class`, so a `\p{L}u` pattern with a
///   capture group is fully DFA-resident.
fn resolve_dfa<'a>(
    prog: &Program,
    flags: u8,
    dfa_cached: Option<&'a crate::dfa::DfaProgram>,
    local: &'a mut Option<crate::dfa::DfaProgram>,
) -> Option<&'a crate::dfa::DfaProgram> {
    let dfa_fast_path = prog.can_dfa && crate::dfa::prog_ops_dfa_safe(prog);
    if dfa_fast_path && dfa_cached.is_none() {
        // RFC 20260711 chunk B — drop poisoned builds (unserveable
        // K-PROPERTY shape); the caller falls to the Pike VM.
        *local = Some(crate::dfa::build_dfa(prog, flags)).filter(|d| !d.poisoned);
    }
    if dfa_fast_path {
        dfa_cached.or(local.as_ref())
    } else {
        None
    }
}

/// Run the anchored DFA at byte offset `st`, selecting the entry state
/// by left context. Returns the matched byte length on hit.
///
/// chunk 8.5 / 8.8 entry-state selection. Use the text-start closure
/// (`dfa.start` — `^` advanced) when the cursor is at a line boundary:
/// byte 0 of the haystack, or — when the pattern's `^`s are multiline
/// (`Program::has_ml_anchor_b`, per-inst m-bits; mixed bits are gated
/// off the DFA at compile) — immediately after a `\n` byte. Otherwise
/// use the mid entries (`^` blocked). Patterns without `Op::AnchorB`
/// dedup the indices, so the branch is free.
///
/// Round 3 Phase B attack #R-A2 — `all_starts_equal` is set by
/// `build_dfa` (and `baked_dfa_view`) when the four anchored start
/// indices collapse to one — patterns without `^` / `\b` / `\B` /
/// multiline-`^`. In that case the `at_line_start` + `prev_is_word`
/// selection is wasted work; jump straight into `dfa_search` at
/// `dfa.start`. For `/\p{L}+/u`-style fixtures this saves ~12 ns/iter.
///
/// chunk 8.6b — when not at a line-start, pick the mid entry whose
/// `LeftByteAttr` matches `s[st-1]`'s class so `Op::WBound` on the
/// first step sees the correct left-byte class. Word class = ASCII
/// `[A-Za-z0-9_]`, mirroring `at_word_boundary`. Patterns without
/// `\b` / `\B` dedup the two mid states down.
fn dfa_probe(dfa: &crate::dfa::DfaProgram, prog: &Program, s: &[u8], st: i64) -> Option<usize> {
    let hay_suffix = &s[st as usize..];
    if dfa.all_starts_equal {
        return crate::dfa::dfa_search(dfa, prog, hay_suffix);
    }
    let at_line_start =
        st == 0 || (prog.has_ml_anchor_b && st > 0 && s[(st - 1) as usize] == b'\n');
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
}

/// Materialise the [`MatchResult`] for a DFA hit spanning `[st..end]`.
///
/// The DFA itself is capture-blind (chunk 9: `Op::Save` is a no-op ε
/// in the closure), so the byte-step traversal found `[st..end]`
/// without populating saves. When the pattern emitted any `Op::Save`
/// ops we run a second-pass Pike VM at the same start with
/// `end_target = end` — its leftmost-first semantics under that length
/// restriction match the DFA's leftmost-longest hit, and the winning
/// thread's snapshot is the JS-spec capture set. Patterns with no SAVE
/// ops skip the second pass — saves stay all-`-1`.
///
/// Round 3 Phase B sub-batch 5 attack #R-A3 — split the no-saves fast
/// path. `prog.has_save == false` (or `want_saves == false`) skips the
/// per-iter saves-row init entirely; `m.saves()` answers an empty
/// slice that reads as all `-1` through `save_slot`.
fn dfa_hit_result(
    prog: &Program,
    s: &[u8],
    st: i64,
    end: i64,
    flags: u8,
    ws: &mut Option<Workspace>,
    want_saves: bool,
) -> MatchResult {
    if !prog_has_save(prog) || !want_saves {
        return MatchResult::no_saves(st, end);
    }
    let ws_ref = ws.get_or_insert_with(|| Workspace::for_program(prog));
    let mut saves = vec![-1i64; ws_ref.arena.stride];
    // Second-pass NFA at exactly the DFA-found window to
    // pull captures out. If the NFA disagrees with the DFA
    // boundary we leave saves all-`-1` rather than report
    // a phantom — this should not happen since the DFA's
    // accept set is derived from the same NFA's `Op::Match`
    // PCs.
    let nfa_end = match_at::vm_match_at(
        prog,
        s,
        st,
        flags,
        ws.get_or_insert_with(|| Workspace::for_program(prog)),
        Some(&mut saves),
        end,
    );
    if nfa_end != end {
        saves.fill(-1);
    }
    MatchResult::with_saves(st, end, saves)
}

/// Pike-VM-only probe at `st` (DFA gate refused). When the program has
/// no `Op::Save` ops we still need the pass for the `end` boundary (no
/// DFA to give us one), but skip the slot array entirely —
/// `MatchResult::no_saves` answers an empty saves row.
fn pike_only_at(
    prog: &Program,
    s: &[u8],
    st: i64,
    flags: u8,
    ws: &mut Option<Workspace>,
    want_saves: bool,
) -> Option<MatchResult> {
    if !prog_has_save(prog) || !want_saves {
        let end = match_at::vm_match_at(
            prog,
            s,
            st,
            flags,
            ws.get_or_insert_with(|| Workspace::for_program(prog)),
            None,
            -1,
        );
        if end >= 0 {
            return Some(MatchResult::no_saves(st, end));
        }
    } else {
        let ws_ref = ws.get_or_insert_with(|| Workspace::for_program(prog));
        let mut saves = vec![-1i64; ws_ref.arena.stride];
        let end = match_at::vm_match_at(prog, s, st, flags, ws_ref, Some(&mut saves), -1);
        if end >= 0 {
            return Some(MatchResult::with_saves(st, end, saves));
        }
    }
    None
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
    ws: &mut Option<Workspace>,
    dfa_cached: Option<&crate::dfa::DfaProgram>,
    haystack_is_ascii: bool,
    want_saves: bool,
) -> Option<MatchResult> {
    let slen = s.len() as i64;
    let mut st = from_pos;
    let mut dfa_built_local: Option<crate::dfa::DfaProgram> = None;
    let dfa_built = resolve_dfa(prog, flags, dfa_cached, &mut dfa_built_local);
    // Rotation 470 — `first_viable_start` skips start positions whose
    // first byte the DFA entry state has no transition for; running
    // the anchored DFA there costs a call and dies one byte in, and on
    // a short haystack that was most of the search (66% of `/hello/i`
    // against a 24-byte line — `examples/match_micro`).
    //
    // It is off whenever the u-flag code-point-boundary guard below
    // could fire, since the skip lands `st` straight on the probe and
    // would step over that guard. Under u flag the haystack has to be
    // non-ASCII for the guard to have anything to say, so the common
    // shapes keep the skip.
    let can_skip_dead_starts = haystack_is_ascii || !crate::parser::unicode_mode(flags);
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
            && crate::parser::unicode_mode(flags)
            && st < slen
            && s[st as usize] & 0xC0 == 0x80
        {
            st += 1;
            continue;
        }
        if let Some(dfa) = dfa_built {
            // Rotation 470 — skip the start positions whose first byte
            // the entry state has no transition for. Running the
            // anchored DFA there would cost a call and die one byte
            // in; `first_viable_start` asks the same question with one
            // load per position. It never advances past `slen`, so a
            // pattern that accepts zero-width at end of input still
            // gets its probe there.
            if can_skip_dead_starts {
                st = crate::dfa::first_viable_start(dfa, s, st as usize) as i64;
            }
            if let Some(n) = dfa_probe(dfa, prog, s, st) {
                return Some(dfa_hit_result(
                    prog,
                    s,
                    st,
                    st + n as i64,
                    flags,
                    ws,
                    want_saves,
                ));
            }
            // Anchored DFA missed at `st`; advance one byte and retry.
            st += 1;
            continue;
        }
        if let Some(m) = pike_only_at(prog, s, st, flags, ws, want_saves) {
            return Some(m);
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
    if crate::parser::unicode_mode(flags) && at < slen && s[at as usize] & 0xC0 == 0x80 {
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
        let mut saves = vec![-1i64; ws.arena.stride];
        let end = match_at::vm_match_at(prog, s, at, flags, &mut ws, Some(&mut saves), -1);
        if end >= 0 {
            Some(MatchResult::with_saves(at, end, saves))
        } else {
            None
        }
    }
}
