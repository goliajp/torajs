# hardev changelog

Incubation versioning, semver-ish. One entry per shipped hardev change.
A pillar item is "shipped" only when its metric in `metrics.md` is
re-measured and the *now* column updated.

## v0.1.14 — 2026-05-19 — hardev efficiency empirically verified on real P7.5 dev work

takagi reopened torajs autorun (full-force P7.5) and asked to test
whether hardev actually speeds dev. Measured on REAL torajs substrate
work (not a synthetic bench):

- **edit→rebuild `tr` (touch torajs-core, `--profile iter`) = 4.46 s**
  under this session's heavy concurrent load, vs **28.5 s** under
  `--release` = **~6.4×** real speedup (idle best-case ~11.4×). The
  order-of-magnitude win (seconds vs half-a-minute) HELD on real P7.5
  substrate iteration. metrics.md §1 edit→rebuild row updated with
  this empirical-on-real-work datapoint; dashboard re-snapshotted
  (live :6002, hardev §03).
- The L2-reopen plan transition (autorun-on-torajs RESUMED, P7.5
  LIVE, hardev → standing tooling) was fully de-drifted across §7
  header / directive block / L4-checklist / L3a — taskq `check.sh`
  caught the half-applied de-drift THREE times (INV-1b version lag,
  stale directive block, stale line-109) and was satisfied only
  after every stale side was corrected (never silenced). taskq
  proven on a real plan-state transition. checker PASS.
- docs/roadmap.md P7 sub-checkboxes de-drifted (P7.1–P7.4 [x] with
  ship refs, P7.5 CURRENT) — `1973e6d`.

P7.5 #1 GROUND done (autorun): probed try/catch/finally vs bun on
spec-§14.13 — 3/4 pass; **real gap found (bun-verified, not
guessed): an Error thrown THROUGH a `finally` then caught loses its
instance type** (`e instanceof Error` = false in tr, true in bun;
finally runs correctly otherwise). That is the P7.5 substrate defect
to fix next.

CHANGELOG/VERSION 0.1.14.

## v0.1.13 — 2026-05-19 — dashboard re-centered torajs-primary (hardev = one supporting section)

takagi (emphatic, 3×): torajs is the product; hardev is a tool whose
value is measured in torajs dev velocity. The dashboard was
hardev-centric — flipped to torajs-primary, restoring labs/pitch.html's
own emphasis (aesthetic/tokens unchanged, only structure/content).

- Render order now Header(torajs) → Hero("TypeScript, compiled to
  silicon" — torajs is an AOT TS→native runtime) → **§01 Progress**
  (the P0→P15 roadmap parsed from docs/roadmap.md + status-grid:
  conformance 629/0/1, current phase, commit, 26/26) → §02 Benchmark
  (torajs vs bun-aot/bun-jsc/node-v8) → **§03 hardev** (one compact
  SECONDARY section: 4 pillars + dev-loop velocity, framed as "what
  hardev buys torajs development").
- snapshot.mjs +`roadmap` (real parse of docs/roadmap.md `### P<N>`
  headings → id/title/status DONE|CURRENT|queued|post-v1.0) +
  `torajs` (commit/phase/conformance). pillars+headline now feed §03.
- Independently audited: render order correct, zero GDS, roadmap
  P0-P6 DONE / P7 CURRENT real from docs/roadmap.md, build rc=0.
- Honest deviation (agent-flagged, verified): docs/roadmap.md P7
  sub-checkboxes are all `[ ]` even though P7.4 shipped (683bd95) —
  a real torajs-progress drift in roadmap.md itself. The snapshot
  reports phase-level truth (P7=CURRENT) and does NOT fabricate
  "P7.4 closed"; roadmap.md's stale sub-boxes are de-drifted
  separately (torajs docs, next commit).

CHANGELOG/VERSION 0.1.13.

## v0.1.12 — 2026-05-19 — taskq checker INV-2 (the gap the v0.1.11 dogfood exposed)

- `taskq/check.sh` +INV-2: **INV-2a** header phase-state ↔
  L4-checklist must affirmatively agree (caught D2: L4 "P7.4 ~95%
  next=frozen" vs header "P7.4 CLOSED"; uses a positive-assertion
  predicate so the L4 line's own de-drift correction-note quoting
  the old stale text doesn't false-positive). **INV-2b** the L2
  directive blockquote must carry NO `hardev v0.1.x` token —
  structurally prevents the decaying parallel-copy drift engine
  (the v0.1.11 dogfood's biggest finding) from regrowing.
- Acceptance caught+fixed a second checker bug (after the v0.1.11
  INV-5 body-grep false-negative): INV-2a's trigger regex required
  `P7.4` before `CLOSED`, but the header phrases it "P7 CLOSED 到
  P7.4" → silent false-N/A (non-enforcement). Rewrote order-
  independent. Verified: PASS on the consistent plan; FAIL on
  synthetic INV-2a (L4 drops the affirmation) and INV-2b (version
  token injected into the directive block).
- check.sh now enforces INV-1a/1b/2a/2b/5. Remaining INV-3/4/6/7
  (hot=actual & pointers cross-ref git+tasks; predicate-form;
  counter re-derivation) = follow-on.

bash tooling, no substrate. CHANGELOG/VERSION 0.1.12.

## v0.1.11 — 2026-05-19 — taskq checker: INV-1a/1b/5 enforced (spec → tooling)

The taskq spec (v0.1.9 INV-1…7) gets its enforcer — same spec→tooling
arc as every pillar.

- `hardev/taskq/check.sh` (bash, no deps, like cleanup/clean.sh):
  parses the live plan source + git, asserts the mechanically-robust
  zero-false-positive subset, exit-coded (session-boundary /
  pre-commit gate, like `bench compare`):
  - **INV-1a** header must reference the current `git HEAD` short sha
    (caught D1: header lagged HEAD by 10 commits).
  - **INV-1b** if focus=hardev, header must name the current
    `hardev/VERSION` (D1-class version lag).
  - **INV-5** a closed-work `## L3a` section's HEADING LINE must
    carry an `ARCHAEOLOGY` marker (caught D3: shipped P7 hot queue
    read as live; the file's own "read L3a top" protocol mis-routes).
- Acceptance caught & fixed a real checker false-negative: INV-5 v1
  grepped the body window for "archaeology", which the banner's own
  explanatory prose matches even after the marker is stripped →
  rewrote to a STRUCTURAL heading-line check (the unambiguous marker
  de-drift adds, a drifted state lacks). Verified: PASS on the
  current consistent plan; FAIL on synthetic INV-1a (stale HEAD) and
  INV-5 (heading reverted to plain) drift.
- Deeper INV-2/3/4/6/7 (cross-ref git/tasks, predicate-form, counter
  re-derivation) = follow-on. taskq/README.md roadmap updated.

bash tooling, no substrate. CHANGELOG/VERSION 0.1.11; README index
+= taskq/check.sh. dashboard snapshot re-run → v0.1.11 live.

## v0.1.10 — 2026-05-19 — hardev web dashboard (devops/starters/web scaffold, no GDS, pitch.html design)

A live webserver surfacing torajs **dev progress + benchmark**, the
visualization layer over the metrics/bench/changelog/git data.

- `hardev/web/` — Vite 8 + React 19 + TS, scaffolded from
  `devops/starters/web` with **ALL GDS stripped** (zero `gds` /
  `@goliapkg/gds` anywhere — verified). Design-led ("frontend-design"):
  the `labs/pitch.html` editorial aesthetic ported verbatim (paper/ink
  tokens, `#d04920` tora-orange, IBM Plex Sans/Mono, the bench-table /
  KPI / pillars / roadmap / status-grid layout), refined for a living
  dashboard. No component library, not Tailwind.
- `scripts/snapshot.mjs` (no-dep Node ESM) reads REAL repo data →
  `src/data.json`: hardev VERSION + CHANGELOG releases, the 4 pillars
  (README + metrics.md), `[M]` headline dev-loop metrics (with a hard
  assert-guard so it fails loudly rather than drift if metrics.md's
  literal numbers change), the newest full bench json (26 cases / 8
  runtimes — currently `2026-05-19-mini-23a6e31.json` @ 23a6e31,
  geomean 4.24× vs bun-aot / 20.15× vs node-v8), conformance 629/0/1,
  recent commits. `build` runs snapshot first; "keep updated" =
  re-run snapshot.
- Independently verified: `bun install` ok · snapshot writes valid
  real data.json · `bun run build` rc=0 clean (snapshot→tsc→prettier
  →vite, dist built) · zero GDS · dev server serves then stops ·
  pitch.html tokens present (41 in index.css). node_modules/dist
  gitignored (not committed).

Pillar-adjacent: this is hardev's reporting/visualization surface
(complements bench 汇报 + taskq governance + metrics-first).

## v0.1.9 — 2026-05-19 — taskq pillar: invariants spec + first application (de-drift live plan)

The 4th and hardest pillar gets its first increment. Same pattern as
every pillar: spec before tooling.

- `hardev/taskq/README.md`: the taskq mandate + **7 machine-checkable
  L1–L4 invariants** (INV-1 single source of truth · INV-2 layer
  agreement · INV-3 hot=actual · INV-4 pointers resolve forward ·
  INV-5 closed→archaeology · INV-6 L4 trigger is a predicate · INV-7
  counters re-derived). Each grounded in a CONCRETE drift observed in
  the live plan source while grounding this pillar (receipts, not
  hypotheticals).
- **First application** — de-drifted the live plan source
  (`memory/project_status_*.md`), which a reader following its own
  protocol ("read L3a top, take #1") would have mis-followed into the
  stale shipped P7 plan:
  - D1: top header self-contradicted (`HEAD 683bd95 … 629-gate
    in-flight` vs the up-to-date directive block) → header now points
    at the authoritative block.
  - D2: L4 checklist said `P7.4 ~95%, next=frozen` while the
    directive said `P7.4 CLOSED` → corrected (P7.4 closed; P7.5 is
    cold because autorun-on-P7 is paused, not "L3a next").
  - D3: `## L3a — Hot 计划` (all shipped P7 prose) → banner-marked
    ARCHAEOLOGY, points at the real hot (hardev / taskq).
  - D4: the `## a-b 实施 fork` section + its dead `#15→#12` pointer
    → banner-marked ARCHAEOLOGY (all shipped).
- `metrics.md` §3 taskq rows advanced (spec'd + first application);
  VERSION 0.1.9.

Pure governance — no language/runtime change, no substrate. Next
taskq increment: a checker (hardev script / bench-sibling) that
parses the plan source and asserts INV-1…7 exit-coded, runnable as
a session-boundary / pre-commit gate. Spec → tooling, same as the
other pillars.

ALL FOUR PILLARS now have shipped tooling/spec: devperf ✅ · bench ✅
v1 core · cleanup ✅ · taskq ✅ first increment.

## v0.1.8 — 2026-05-19 — cleanup pillar: close the devperf-#1 coverage gap

First cleanup-pillar increment (was zero tooling beyond the inherited
script).

- `hardev/cleanup/clean.sh` §5 now also reclaims `target/iter` (the
  hardev devperf-#1 fast-iteration profile cache — 195 MB measured,
  regenerable: conformance rebuilds it ~18 s cold / ~2.5 s
  incremental). It was a real coverage gap: devperf #1 introduced a
  large regenerable subtree the cleaner could not see. `target/debug`
  / `target/doc` stay (pure waste); `target/release` stays GUARDED
  (the live ship/bench-required binary — bench B0 + runners hardcode
  it). Verified in dry-run: target/iter listed (~195 MB), release in
  the never-cleaned skip-list, rc=0, dry-run still the default.
- Rebranded the script header/banners `.dev/clean.sh` → `hardev
  cleanup`.
- `metrics.md` §2: junk-source-coverage metric advanced (target/iter
  covered); cleanup-invocation metric stated honestly — `--force` is
  operator-invoked-under-disk-pressure BY DESIGN, not a gap;
  auto-running it to tick a metric would delete useful cache for no
  reason, contradicting the pillar's own philosophy.
- VERSION 0.1.8.

Pillar status: devperf ✅ (lever found+fixed) · bench ✅ v1 core
(B0/B1/B1b/B2/B2b) · cleanup ✅ first increment (coverage gap closed;
hook automation deferred to Claude Code settings per README) · taskq
= still prose-only (next).

## v0.1.7 — 2026-05-19 — bench B2b SHIPPED: artifact-precheck (seconds for tr-unchanged)

The per-commit bench gate is now SECONDS when the machine code is
unchanged, with zero coverage loss.

- `bench::artifact_only` (compile once, no hyperfine, stat output
  size) + `compare::load_artifacts` (reuses the compare parser) +
  main.rs `--vs <baseline.json>` precheck.
- If every selected torajs artifact is byte-identical to baseline →
  machine code unchanged → no perf regression is physically possible
  → ALL timed runs skipped, exit 0 in ~2 s (measured 1.91 s vs the
  ~10 min full run). If ANY artifact differs / is unknown → list
  them and FALL BACK to the full timed measurement (coverage never
  reduced — first hard rule). Safe by construction: skip only when
  provably no codegen change.
- Verified both branches: PASS (artifacts identical → SKIPPED 1.91 s);
  fallback (throw-catch-100k vs 8b73988 real +416 → "1 changed →
  falling back to FULL timed" → timed runs proceed). fmt clean,
  0-warn, no substrate.
- `optimization-backlog.md` B2b → DONE; `metrics.md` §4
  per-commit-gate row → seconds-for-unchanged; VERSION 0.1.7.

Bench pillar v1 core COMPLETE: B0 (always-fresh ship binary) · B1
(machine compare verdict) · B1b (N-run median/MAD) · B2 (--self
per-commit scope) · B2b (artifact-precheck seconds). Remaining bench
= B3 (phase-tracking coverage; needs an active language phase, P7
paused) · B4 (machine hygiene + methodology, polish).

## v0.1.6 — 2026-05-19 — bench B2 SHIPPED: `--self` per-commit fast path

- `bench run --self`: restrict to the torajs runtimes
  (torajs / torajs-run), dropping bun/node/go/rust/python — those are
  the SOTA cross-runtime comparison (a phase-close concern), not a
  per-commit regression gate. ~3-4x faster per-commit.
- Coverage NOT reduced: the regression target is torajs vs its own
  baseline; phase-close still runs the full 8-runner matrix (first
  hard rule). An explicit `--runtime` always overrides `--self`. A
  per-commit-scope notice is printed so a `--self` run is never
  mistaken for a phase-close full run.
- Verified: `run fib40 --self` → only fib40×{torajs,torajs-run} +
  notice; `--runtime bun-jsc` overrides (no notice, bun-jsc only);
  --help lists it. fmt clean, 0-warn, no substrate.
- artifact-precheck (skip timed runs when artifact_bytes unchanged
  vs a baseline → seconds) split out as **B2b** (follow-on).
- `optimization-backlog.md` B2 → DONE (+ B2b filed); `metrics.md`
  §4 per-commit-gate row updated; VERSION 0.1.6.

## v0.1.5 — 2026-05-19 — bench B1b SHIPPED: native N-run aggregation (median + MAD)

- `bench run --runs N` (default 1, fully backward-compatible). N
  **full interleaved passes** (whole case×runner matrix per pass,
  repeated N times) so the median samples machine-state variance
  across time (the historical "3 full-suite runs" intent), not N
  back-to-back runs of one cell.
- Per-cell aggregation (`report::aggregate`): `run_ms` = median,
  `run_stddev_ms` = **MAD** (robust spread, barely moved by a single
  mac thermal spike), `compile_ms` = median, `artifact_bytes` =
  shared value if all identical else median (benign ±N linker drift,
  already handled by `bench compare`), `status` = worst (a single
  failing pass is never hidden). `Report.runs` records aggregation
  depth so readers/`bench compare` interpret the spread correctly.
- Kills the same-name-overwrite + agent-log-parsing workflow: one
  statistically-sound json per invocation, directly consumable by
  `bench compare`.
- Verified: `run fib40 --runtime torajs --runs 3` → json `runs:3`,
  fib40 median 176.194 ms / MAD 4.0612; `bench compare` consumes it;
  no flag → `runs:1`, single-pass behavior unchanged. fmt clean,
  0-warn. bench-harness tooling, no substrate.
- `optimization-backlog.md` B1b → DONE; `metrics.md` §4 multi-run
  row → SHIPPED; VERSION 0.1.5.

## v0.1.4 — 2026-05-19 — bench B1 SHIPPED: `bench compare` machine regression verdict

The reporting gap takagi named ("agent hand-runs ad-hoc python to
eyeball two json files — not a command, not reproducible, not
in-repo") is closed.

- `bench/harness/src/compare.rs` + main.rs wiring: `bench compare
  <baseline.json> <current.json> [--allow-artifact-delta
  case:runtime,…]`. Encodes the empirically-established methodology:
  **artifact_bytes is the HARD GATE** (deterministic; any per-case
  change = regression suspect → exit 1 unless justified);
  **run_ms is noise-aware** (only classified where the same case's
  artifact_bytes ALSO changed; identical artifact ⇒ run delta is
  noise by construction, informational only).
- Verified: reproduces the earlier hand-python finding exactly
  (8b73988→76ace15: torajs `array-sum-1m -16`, `throw-catch-100k
  +416`; 94 identical); unjustified → VERDICT FAIL exit 1;
  `--allow-artifact-delta` → PASS exit 0; identical files → 0 delta
  PASS. fmt clean, 0-warn. bench-harness tooling, no substrate.
- N-run native aggregation (the same-name-overwrite fix) split out
  as **B1b** (follow-on, before B2).
- `optimization-backlog.md` B1 → DONE (+ B1b filed); `metrics.md`
  §4 regression-verdict row → SHIPPED; VERSION 0.1.4.

## v0.1.3 — 2026-05-19 — bench B0 SHIPPED: bench always measures the current ship binary

Closes the operational footgun devperf #1 introduced (conformance no
longer side-produces `target/release/tr`).

- `bench/harness/src/main.rs`: `run_cmd` now calls `ensure_release_tr`
  before any case — `cargo build --release -p torajs-cli` (cwd =
  workspace), fail-fast on build error, verify `target/release/tr`
  exists. Auto-build (not pure fail-fast) chosen: idempotent, zero
  manual step, bench can never silently measure a stale/missing
  binary (first hard rule: bench must measure the real ship artifact).
- Verified: stale release-tr (last build was `target/iter/tr` from
  devperf #1) → B0 rebuilt it (30.5 s) then benched correctly; fresh
  → guard no-ops in 0.08 s. fmt clean, 0-warn. bench-harness tooling,
  no substrate (no conformance gate needed).
- `optimization-backlog.md` bench B0 → DONE; VERSION 0.1.3.

## v0.1.2 — 2026-05-19 — devperf #1 SHIPPED: fast iteration profile (~11.4× inner loop)

First hardev pillar tooling shipped. The 28.5 s inner-loop tax found
in v0.1.1 is gone.

- `Cargo.toml`: added `[profile.iter]` (inherits release; `lto=false,
  codegen-units=256, opt-level=1, strip=false`). `[profile.release]`
  untouched.
- `conformance/runner/main.rs`: builds tr with `--profile iter`
  instead of `--release` (+ fallback path / docs).
- **Measured**: touch `torajs-core` → rebuild `tr` = **2.49 s** (was
  28.5 s under `--release`) = **~11.4×**. Cold all-deps iter build
  18.5 s one-time; no-op 0.05 s.
- **Correctness-equivalence empirically proven**: full conformance
  with the iter-profile tr = **629 / 0 / 1** (0 FAIL). opt-level / LTO
  / codegen-units are semantics-invariant — an `iter` tr is
  byte-for-byte stdout-equal to a `release` tr on every case. Same
  coverage, same byte-equal verdict, first hard rule intact.
- **bench + ship unaffected**: `target/iter/tr` and `target/release/tr`
  are physically separate; bench runner descriptors hardcode
  `target/release/tr`, unchanged.
- **Operational contract introduced (not silent)**: conformance no
  longer incidentally produces `target/release/tr`. A bench run MUST
  be preceded by `cargo build --release -p torajs-cli` or it measures
  a stale/missing binary (= measuring the wrong thing). Filed as
  **bench B0** (highest bench prerequisite — fail-fast if release-tr
  is stale).
- `metrics.md` §1 edit→rebuild row → SHIPPED/measured; VERSION 0.1.2.

Combined with the P1 conformance parallelization (~10×), the torajs
dev loop is qualitatively transformed: edit → (2.5 s build) →
(~3 min full 629-case correctness) instead of (28.5 s build) →
(~30 min serial).

## v0.1.1 — 2026-05-19 — devperf P0 root-caused (sccache myth busted, true lever found)

First metrics-driven investigation. Outcome: the sccache "0 %" was not
a misconfig to fix — it is **structural** (machine-global shared server;
sccache never caches `bin`/proc-macro/build-script; a changed
`torajs-core` source is a *correct* miss). The prior `environment.md`
§3 "sccache is the real build lever (3285 hits)" was a global
cross-project snapshot mis-recorded as a torajs conclusion.

**The real lever, found because we measured**: `[profile.release]`
(`lto="fat", codegen-units=1`, max-opt ship profile) is reused for
*every* iteration build. Measured: **touch `torajs-core` → rebuild
`tr` = 28.5 s** (no-op = 0.05 s). That tax × hundreds/session was the
hidden dominant dev-loop cost.

- `environment.md` §3 rewritten: refuted claim → root-caused truth +
  the "don't write a global tool's global snapshot into project ground
  truth" lesson.
- `metrics.md` §0 + §1: re-measured. New headline metric
  **edit→rebuild wall = 28.5 s [M]** (v1 target ≤ 5 s); sccache hit
  rate dropped as a torajs lever (was a misconception, not a target).
- `optimization-backlog.md`: **devperf #1** filed — fast iteration
  profile (lto=off/cgu=many/low-opt) for functional+conformance work
  (opt-level is semantics-invariant → 629/0/1 still proves
  correctness, coverage unchanged); bench+ship keep fat-LTO release.
  Machine-decidable acceptance attached.

No code/config changed yet (P0 = root-cause + plan). devperf #1
(the profile fix) is the next autorun item.

## v0.1.0 — 2026-05-19 — incubation scaffold + observability baseline

Named, versioned, Rust-specialized R&D-support framework, incubated
in-project inside `torajs`. Establishes identity and the measurement
foundation; **no pillar tooling shipped yet** (deliberate: measure
before optimize).

- Promoted the standing `.dev/` R&D-environment theme into the named
  framework **hardev** (`git mv .dev hardev`, history preserved):
  `.dev/README.md` → `hardev/README.md` (rewritten as the charter),
  `ENVIRONMENT.md` → `environment.md`, `OPTIMIZATION.md` →
  `optimization-backlog.md`, `clean.sh` → `cleanup/clean.sh`.
- Charter (`README.md`): identity, the **first hard rule** (no
  optimization/automation may reduce verification coverage or
  correctness), the **four pillars** (devperf / cleanup / taskq /
  bench), incubation + Rust-specialization stance, taskq L1–L4
  governance charter.
- **`metrics.md`**: observability baseline with provenance tags
  (`[M]` measured / `[A]` assumed-flagged / `[D]` design target),
  per-pillar now → v1 → v2 targets. This is the v0.1.0 centerpiece.
- **Headline finding from measuring**: `sccache --show-stats` →
  **0.00 % hit rate (85/85 Rust misses)**, contradicting the prior
  `environment.md` §3 "sccache is the real build lever (3285 hits)"
  assumption. `environment.md` §3 amended to flag the claim
  `[A]`-unverified pending root-cause (devperf P0). Establishing this
  contradiction is exactly the value of metrics-first.
- CLAUDE.md "Project Structure" + the dev-env pointer updated
  `.dev/` → `hardev/`.

Context: shipped immediately after P7.4 closed (`683bd95`, conformance
629/0/1) and autorun was paused by takagi to focus on the R&D-support
framework — torajs's scale ("too large/important/complex/hard")
mandates advanced tooling before more language surface.

### v1 — target (metric values, not features)

Every pillar has working tooling and its `metrics.md` v1 column met:
sccache root-caused to ≥80 % hits; `bench compare` is a real
machine-judged command (kills ad-hoc python); per-commit bench gate in
seconds (`--self` + artifact-precheck) with phase-close full run
unchanged; cleanup hook-automated at session boundaries; taskq items
layer-tagged with a machine-evaluable L4 predicate + drift detector.

### v2 — target

Hardened & extractable beyond torajs: statistical noise-band regression
verdict in CI, machine-hygiene-gated trustworthy run_ms, coverage that
auto-tracks the active phase, taskq invariants machine-checked on every
edit, cleanup self-auditing for unknown large dirs.
