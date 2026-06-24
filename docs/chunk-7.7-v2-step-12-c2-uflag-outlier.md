# chunk 7.7 v2 step 12 C2 — uflag-100k outlier Phase A decomposition

Tracking note for the single Phase C bench acceptance outlier
(`regex-dfa-uflag-100k`).

## Acceptance status

C2 Phase C SHIPPED 2026-06-24 (commit chain `3e817c07 → 4b039d7a →
3e2f003b → 171d1226`). 6/7 regex-dfa-* fixtures land at 1.86×-2.03× vs
bun-aot 1.3.14 (3-run median on mini, host=mini), closing the
handoff 600× gap to ~2×. **uflag-100k remains 10.35×** (149.96 ms vs
14.49 ms bun-aot) — single outlier that prevents the geomean from
reaching the design-principles §1 1.5× ceiling.

## Confirmed: baked path is wired

The outlier is NOT a ssa_lower-side AOT-gate miss. `tr build` the
fixture (`/\p{L}+/u`) emits the baked entry as expected:

- `otool -tv /tmp/uflag-probe-bin | grep -c compile_from_static_dfa` = **2**
  (both `s.match(re)` sites lower to the baked entry point)
- `otool -tv /tmp/uflag-probe-bin | grep -c 'regex_compile\b'` = **0**
  (no fallback path)
- Binary size 1.78 MB vs 1.51 MB baseline = +270 KB DFA payload
  ≈ 256 DfaState (1060 bytes / state)

Runtime side `RegExp::baked_dfa_view` returns `Some(DfaProgram)` whose
`states` is `DfaStates::Static(&'static [DfaState])` from
`.rodata` (chain-LC rebased) — `dfa_search` walks the same byte-step
loop the other 6 cases walk.

## Root cause (hypothesis, not verified yet)

`/\p{L}+/u` exercises the chunk-10d `utf8_class_expand` rewrite of
the `\p{L}` Unicode-letter class. Per chunk-10d's design note (in
`torajs-regex/src/utf8_class_expand.rs`), unsafe Unicode classes
under the u-flag get rewritten compile-time into a byte-level
`Alt(Concat([byte, byte, byte, ...]))` over `byte_only` `CharClass`
instructions. The DFA byte-step then walks the expanded sub-graph
verbatim. Empirically this produces:

- DFA state count ~256 (vs ~10-30 for the 6 fixed cases — direct
  evidence from binary size delta: 270 KB / 1060 bytes ≈ 256
  vs ~10 KB / 1060 ≈ 10)
- Per-input-byte transition lookup cost is **the same** (one
  `transitions[byte]` indirection); but BFS-state cache footprint
  is ~25× larger
- bun-aot also walks the same Unicode coverage but in **14.5 ms**
  — JSC YARR JIT specialises the DFA executor into native code,
  amortising the L1-D cache misses across a much wider issue
  window than a byte-step interpreter loop

## Why not a Phase C narrow polish

Per `rules/torajs-perf-decomposition.md`:

1. The hot path is `dfa_search` byte-step interpreter, not the
   ssa_lower-side AOT gate (which works correctly here).
2. bun-aot 14.5 ms ÷ tora 150 ms = 10× — well above the 1.5×
   "idiomatic Rust vs C" ceiling. The framework says any > 1.5×
   gap is **an abstraction wasting CPU**, not the language. The
   abstraction here is the byte-step `transitions[byte]` lookup
   running through a `&[DfaState]` slice + `Deref` indirection
   without `#[inline]`-promoted state index advancement.
3. Two-round polish on the executor without bench-step decomposition
   would land the "no animation" pattern called out in
   `§1 Self-deception detector` → switch to decomposition before
   polish.

## Phase A decomposition checklist (next phase, not this session)

Per `torajs-perf-decomposition.md` §1 the next session should:

1. **Clone bun source** + read JSC YARR DFA executor
   (`bun/src/bun.js/bindings/wtf/text/StringRegExpEdit...` —
   exact path TBD per Phase A read). Identify which abstraction
   bun has elided that tora hasn't.
2. **Decompose** the `dfa_search_*` family into stages: byte-load
   from `hay[i]` / `transitions[byte]` index / `state = states[idx]`
   load / `is_accept` check / increment `i`. Side-by-side stage
   µs estimate vs JSC's equivalent loop.
3. **Top-N attacks** prioritised by µs gain:
   - inline `Deref` lowering (the `DfaStates::Owned` /
     `DfaStates::Static` enum match site)
   - vectorise transitions table lookup (SIMD scatter / batched
     prefetch — bun's YARR likely doesn't, but worth measuring)
   - hoist `&[DfaState]` raw pointer outside the loop (manual
     `as_ptr()` + bounds-check elision via `unsafe` block — last
     resort, only if the safe path measurably bottlenecks)
4. **Attack ship** in worktree isolation; bench validate batched
   (single-attack gains will be sub-noise per the methodology).

## Scope estimate

- Phase A read + decomposition note: 4-8 hours of read + 1 day write
- Phase B attack ship: 1-2 days (3-5 attacks, sub-noise unitarily,
  cumulative ≥ 2× gain target)
- Total: ~1 sprint (3-4 days wall) — multi-session, post-rotation
  scope

## Why this isn't blocking v1.0

- Phase C SIGBUS acceptance HIT (regex-021 × 500 × 2 = 0 / 0 SIGBUS)
- Phase C bench acceptance HIT 6/7 — uflag is a single-cell outlier
- v1.0 release gate (`roadmap.md` §1493) is substrate-completeness +
  conformance gate green + bench-tr 0 regression on typed-tier.
  uflag-100k is NOT on the typed-tier — it's regex perf bench. The
  P14+ post-v1.0 trunk can absorb the dfa_search executor polish
  without holding v1.0.

## Linked

- `.claude/rfcs/20260624-chunk-7.7-v2-step-12-c2-aot-dfa/design.md` §10 (Phase C ship close report)
- `bench/results/2026-06-24-mini-171d122*.json` (7-fixture acceptance data)
- `.claude/rules/torajs-perf-decomposition.md` (Decomposition + Attack methodology)
- `crates/torajs-regex/src/utf8_class_expand.rs` (chunk 10d rewrite rule)
- `crates/torajs-regex/src/dfa/search.rs` (byte-step executor)
