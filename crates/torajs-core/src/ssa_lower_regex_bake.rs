//! V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-6 — host-side DFA bake.
//!
//! `try_bake_regex_dfa` runs the same parse → resolve_backrefs →
//! compile → analyze chain `__torajs_regex_compile_from_static_dfa`
//! uses at run-time (see `torajs_regex::regex::compile_aot`), but at
//! `tr build` time, for capture-free + DFA-eligible literal regex
//! patterns. On success it serialises each `#[repr(C)] DfaState`
//! byte-by-byte and pushes a `BakedRegexEntry` so the link layer
//! (`user_regex_baked_layout`, C-5b/c) can lay the `BakedDfaMeta` +
//! `[DfaState; N]` payload into `__DATA_CONST`. The `Expr::Regex`
//! lowering arm then emits a `Call(regex_compile_from_static_dfa,
//! [GlobalRef(meta_sym), pat, flag])` instead of `regex_compile`, so
//! the run-time path skips `build_dfa` AND the
//! `UnsafeCell<Option<DfaProgram>>` lazy borrow that produced the
//! chunk-7.6 / chunk-7.7 SIGBUS.
//!
//! Returns `Some(index)` into `baked_regex_buf` on success; `None`
//! when the pattern parses with capture groups, fails to parse, is
//! DFA-ineligible (lookaround / backref), or contains opcodes the DFA
//! substrate explicitly rejects.

use crate::ssa::BakedRegexEntry;

/// Try to host-build a baked DFA payload for the literal pattern.
///
/// Capture-free + DFA-eligible literal regexes win — the runtime
/// surface for these patterns (`.test()`, `.search()`,
/// conditional `if (re.test(x))`, lex-style scans) walks the DFA
/// only; the Pike VM second pass is for capture extraction and isn't
/// reached. Ineligible patterns + `new RegExp(...)` dynamic-arg form +
/// patterns with capture groups continue routing through
/// `__torajs_regex_compile` so capture semantics stay intact.
pub(crate) fn try_bake_regex_dfa(
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    pattern: &str,
    flags: &str,
) -> Option<u32> {
    use torajs_regex::compiler::compile;
    use torajs_regex::dfa::{DfaState, analyze, build_dfa, prog_ops_dfa_safe};
    use torajs_regex::flags::parse_flags;
    use torajs_regex::parser::Parser;
    use torajs_regex::program::{Inst, Program};
    use torajs_regex::resolve::resolve_backrefs;

    // Strict flag parse (Option): `None` = SyntaxError from duplicate
    // or unknown letter — no bake, runtime `regex_compile` will hit
    // the same `None` and route through the rejected-stub path.
    let flag_bits = parse_flags(flags.as_bytes())?;
    // ES §22.2.3.1 — `u` + `v` is a SyntaxError; a baked table
    // would be dead weight.
    if flag_bits & torajs_regex::parser::RE_FLAG_U != 0
        && flag_bits & torajs_regex::parser::RE_FLAG_V != 0
    {
        return None;
    }
    let mut parser = Parser::new(pattern.as_bytes(), flag_bits);
    let mut root = parser.parse()?;
    if parser.err() {
        return None;
    }

    // Capture-using literals route through __torajs_regex_compile
    // (runtime DFA-then-Pike pair); the baked path is capture-free
    // only — DFA loses save info by construction and the Pike second
    // pass needs a runtime Program anyway.
    if parser.n_captures > 0 {
        return None;
    }

    let names = parser.names.clone();
    let n_caps = parser.n_captures;
    if !resolve_backrefs(&mut root, &names, n_caps, flag_bits) {
        return None;
    }

    // Mirrors `regex/compile.rs` can_dfa gates: a multiline `$`
    // (per-inst m-bit, regexp-modifiers) is VM-only — the runtime
    // RegExp for such a literal carries `can_dfa = false`, so a baked
    // table would be dead weight.
    if !analyze(&root).is_eligible() || torajs_regex::dfa::tree_contains_ml_anchor_end(&root) {
        return None;
    }

    let mut prog = Program::new();
    compile(&mut prog, &root, flag_bits);
    prog.emit(Inst::match_accept());
    prog.can_dfa = true;

    if !prog_ops_dfa_safe(&prog) {
        return None;
    }

    // regexp-modifiers — mixed `^` m-bits gate the DFA off at
    // runtime (`regex/compile.rs`); don't bake dead tables for them.
    let mut any_ml = false;
    let mut any_plain = false;
    for inst in &prog.insts {
        if inst.op == torajs_regex::program::Op::AnchorB as u8 {
            if inst.pad as u8 & torajs_regex::parser::RE_FLAG_M != 0 {
                any_ml = true;
            } else {
                any_plain = true;
            }
        }
    }
    if any_ml && any_plain {
        return None;
    }
    prog.has_ml_anchor_b = any_ml;

    // ABI-locked: `DfaState` is `#[repr(C)]`, 1072 bytes on
    // aarch64-apple-darwin (Round 3 Phase B sub-batch 4 attack #R-J v2,
    // up from 1060 — 12-byte `PendingClass` triple appended for K-
    // PROPERTY cp-step support). `dfa_state_repr_c_layout_locked` in
    // torajs-regex pins the layout — if it ever drifts the test
    // breaks before the byte emitter sees a corrupt payload. The
    // run-time reader reconstructs `&[DfaState]` from the same byte
    // sequence (see `RegExp::baked_dfa_view`).
    let dfa = build_dfa(&prog, flag_bits);
    // RFC 20260711 chunk B — poisoned builds (unserveable K-PROPERTY
    // shape, e.g. `\p{L}|\p{N}` disjunction) never bake; the runtime
    // Pike VM path serves those literals.
    if dfa.poisoned {
        return None;
    }
    let states_slice: &[DfaState] = &dfa.states;
    let state_size = core::mem::size_of::<DfaState>();
    let mut payload: Vec<u8> = Vec::with_capacity(states_slice.len() * state_size);
    for state in states_slice {
        // SAFETY: `DfaState` is `#[repr(C)]` POD (no pointers, no
        // drop) and has no implicit padding — every byte between
        // `transitions` and `pending_class` belongs to a declared
        // field, `_pad` included. That is what makes reading the
        // struct's byte representation through `*const u8` sound:
        // every byte read is initialised, and the sequence matches
        // the layout the link emitter writes into __DATA_CONST and
        // the runtime reader walks.
        //
        // The padding must stay declared. `Default` initialises
        // fields, not holes, so an implicit padding byte read here is
        // uninitialised memory — it took whatever the allocator last
        // left there and carried it into the binary, which is how the
        // same source built twice produced two different artifacts.
        let bytes = unsafe {
            core::slice::from_raw_parts(state as *const DfaState as *const u8, state_size)
        };
        payload.extend_from_slice(bytes);
    }

    let idx = baked_regex_buf.len() as u32;
    baked_regex_buf.push(BakedRegexEntry {
        index: idx,
        states_payload: payload,
        states_len: states_slice.len() as u32,
        start: dfa.start,
        start_mid: dfa.start_mid,
        start_mid_word: dfa.start_mid_word,
        start_mid_nonword: dfa.start_mid_nonword,
        // Round 3 Phase B attack #R-E — host-baked mirror of the
        // runtime-built DFA's `any_accept_before_byte` flag. The
        // executor reads it to gate the per-byte `accept_before_byte`
        // mask check; for patterns without `\b` / `\B` / multiline-`$`
        // this is `false`, saving ~35 ns/iter.
        any_accept_before_byte: dfa.any_accept_before_byte,
    });
    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_free_eligible_literal_pushes_entry() {
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"hello\d+", "");
        assert_eq!(idx, Some(0));
        assert_eq!(buf.len(), 1);
        let e = &buf[0];
        assert_eq!(e.index, 0);
        assert!(e.states_len > 0);
        // Round 3 Phase B sub-batch 4 attack #R-J v2 — DfaState size
        // 1060 → 1072 bytes (12-byte PendingClass triple appended) on
        // aarch64-apple-darwin.
        assert_eq!(e.states_payload.len(), e.states_len as usize * 1072);
    }

    #[test]
    fn capture_using_literal_returns_none() {
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"(hello)\d+", "");
        assert_eq!(idx, None);
        assert!(buf.is_empty());
    }

    #[test]
    fn backref_returns_none() {
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"(\w+)\1", "");
        assert_eq!(idx, None);
        assert!(buf.is_empty());
    }

    #[test]
    fn lookahead_returns_none() {
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"hello(?=world)", "");
        assert_eq!(idx, None);
        assert!(buf.is_empty());
    }

    #[test]
    fn invalid_pattern_returns_none() {
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"[", "");
        assert_eq!(idx, None);
        assert!(buf.is_empty());
    }

    #[test]
    fn anchored_word_class_eligible() {
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"^\w+", "");
        assert_eq!(idx, Some(0));
        assert_eq!(buf.len(), 1);
        assert!(buf[0].states_len > 0);
    }

    #[test]
    fn multiple_literals_get_sequential_indices() {
        let mut buf = Vec::new();
        let i0 = try_bake_regex_dfa(&mut buf, r"foo", "");
        let i1 = try_bake_regex_dfa(&mut buf, r"bar", "");
        let i2 = try_bake_regex_dfa(&mut buf, r"baz", "");
        assert_eq!((i0, i1, i2), (Some(0), Some(1), Some(2)));
        assert_eq!(buf.len(), 3);
        for (i, e) in buf.iter().enumerate() {
            assert_eq!(e.index, i as u32);
        }
    }

    #[test]
    fn iflag_capture_free_eligible() {
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"hello", "i");
        assert_eq!(idx, Some(0));
        assert_eq!(buf.len(), 1);
    }

    #[test]
    fn uflag_pclass_eligible_small_payload() {
        // Round 3 Phase B sub-batch 4 attack #R-J v2 — K-PROPERTY now
        // skips chunk-10d Alt-of-Concat expansion. The DFA build pre-
        // emits a pending K-PROPERTY state with a fixed-size triple;
        // the post-pending state holds the `+` Kleene loop body. The
        // literal IS still DFA-eligible (just much smaller than the
        // ~256 chunk-10d expansion), so `try_bake_regex_dfa` still
        // returns `Some(idx)` and bakes a tiny payload (~4 states for
        // `\p{L}+/u`). The closed gap is the central perf win.
        let mut buf = Vec::new();
        let idx = try_bake_regex_dfa(&mut buf, r"\p{L}+", "u");
        assert_eq!(idx, Some(0));
        assert!(
            buf[0].states_len > 0 && buf[0].states_len <= 10,
            "uflag \\p{{L}}+ should collapse to ≤ 10 DFA states with \
             Path A v2 (single pending K-PROPERTY state + post-pending \
             loop), got {}",
            buf[0].states_len
        );
    }

    /// Ground-truth DFA state count vs run-time delta for the 7
    /// regex-dfa-* fixtures (mini, host benchmark @ 171d1226 vs
    /// bun-aot 1.3.14). Locked here so future polish of the byte-
    /// step executor can A/B-compare without manually rebuilding
    /// each pattern's DFA.
    ///
    /// Round 3 Phase B sub-batch 4 attack #R-J v2 — the assertion
    /// direction inverts: uflag's chunk-10d Alt-of-Concat expansion
    /// is replaced by a fixed-size pending K-PROPERTY state, so
    /// uflag's state count is now SMALLER (was ≥10× anchored, now
    /// ≤3× anchored). Bumping back to ≥10× would mean Path A
    /// reverted; checking ≤3× catches both that and any future
    /// regression that re-introduces chunk-10d expansion for
    /// K-PROPERTY classes.
    #[test]
    fn ground_truth_states_len_per_fixture() {
        let fixtures = [
            (r"^\w+", "", "anchored"),
            (r"hello", "i", "iflag"),
            (r"\w+\b", "", "wbound"),
            (r"hello$", "", "eflag"),
            (r".+", "s", "dotall"),
            (r"[abc]+", "", "safeclass"),
            (r"\p{L}+", "u", "uflag"),
        ];
        let mut counts = Vec::new();
        for (pat, fl, name) in fixtures {
            let mut buf = Vec::new();
            let idx = try_bake_regex_dfa(&mut buf, pat, fl);
            assert_eq!(idx, Some(0), "{name} ({pat}, {fl}) should be eligible");
            counts.push((name, buf[0].states_len));
        }
        // regexp-modifiers — a multiline `$` (global m or `(?m:…)`)
        // is VM-only at runtime (`can_dfa = false`), so the bake now
        // declines it instead of emitting a dead table. Mixed `^`
        // m-bits decline for the same reason.
        let mut buf = Vec::new();
        assert_eq!(try_bake_regex_dfa(&mut buf, r"hello$", "m"), None);
        assert_eq!(try_bake_regex_dfa(&mut buf, r"(?m:^a)|^b", ""), None);
        // Uniform-multiline `^` (no `$`) stays bake-eligible.
        assert!(try_bake_regex_dfa(&mut buf, r"^hello", "m").is_some());
        let uflag_count = counts.last().unwrap().1;
        let anchored_count = counts[0].1;
        assert!(
            uflag_count <= anchored_count * 3,
            "Round 3 Path A v2 — uflag K-PROPERTY now skips chunk-10d \
             Alt-of-Concat expansion; state count drops to within 3× \
             of anchored. Got uflag={uflag_count}, \
             anchored={anchored_count}. A regression to ≥10× would \
             mean Path A reverted.",
        );
    }
}
