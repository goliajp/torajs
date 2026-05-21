# hardev metrics — observability baseline & version targets

> **Why this file exists**: measure before you optimize. hardev does not
> touch a pillar until that pillar has a metric, a measured *now*, and a
> target for *v1* / *v2*. This file is the single source of truth for
> "where are we, where are we going". Every number carries a
> **provenance tag**: `[M]` measured this session (cite how), `[A]`
> previously *assumed* and now flagged for re-measurement, `[D]` design
> target (not yet measured). No untagged numbers — an untagged metric is
> a hallucination risk.
>
> Versioning: **v0.1.0** = incubation scaffold + this baseline. **v1** =
> every pillar has working tooling and its v1 metric target met. **v2** =
> hardened / statistical / extractable beyond torajs. Targets below are
> metric values, not feature lists.

## 0. Headline finding from establishing the baseline

Establishing metrics *immediately* paid off (this is the point):

- **sccache hit rate = 0.00 %** `[M]` (`sccache --show-stats`, this
  session: 0 hits / 85 misses; **`Non-cacheable calls 1373`,
  reason `crate-type`**). **ROOT-CAUSED** (devperf P0, same session):
  (a) sccache is a **machine-global shared server** — `--show-stats`
  counts all projects (caught it serving a *different* project `frz`
  mid-measurement); the old "3285 hits" was a global cross-project
  snapshot, never a torajs signal. (b) **Structural**: sccache only
  caches lib/rlib, **never `bin`/proc-macro/build-script**; torajs's
  hot rebuild is the `tr` **bin** + changed-`torajs-core`-source
  (a *correct* miss). **sccache structurally cannot accelerate
  torajs's inner loop — not a bug.** (c) **The real lever was hidden
  by this misconception**: `[profile.release]` = `lto="fat",
  codegen-units=1` (max-opt **ship** profile) is used for *every*
  iteration build → **measured 28.5 s to rebuild `tr` after touching
  `torajs-core`** `[M]`. That 28.5 s × hundreds/session is the
  dominant dev-loop tax. Fix = a fast iteration profile for
  functional+conformance work (semantics are opt-level-invariant →
  650/0/1 still proves correctness, coverage unchanged — was 629/0/1
  at v0.1.0 first-measure, now 650 post P9.3/P9.4/P9.5 fixtures), bench+ship
  keep fat-LTO release. This is *exactly* why metrics precede
  optimization: a global tool's global snapshot had been written into
  ground truth as a torajs-specific conclusion, hiding the true lever.

## 1. devperf — dev-loop performance

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| full conformance wall (650 cases) | **~3.0–3.5 min** `[M]` parallel 8-worker (174–208 s ×N this session at 629 cases; shipped `6ab22f9`, was ~30 min serial; case count now 650 post P9.3/P9.4/P9.5 — wall holds linear) | ≤ 2 min (artifact-precheck skips timed re-verify when tr unchanged) `[D]` | ≤ 30 s for the common "tr unchanged" case `[D]` |
| **edit→rebuild `tr` wall** (THE inner-loop metric) | **2.49 s idle / 4.46 s under heavy load** `[M]` ✅ devperf #1 SHIPPED — `[profile.iter]`; was **28.5 s** under `--release`. **Re-measured on REAL P7.5 dev work (2026-05-19, takagi "test if hardev actually speeds dev"): 4.46 s** under this session's heavy concurrent load = **~6.4×**; idle best-case ~11.4×. The order-of-magnitude gain (seconds vs half-a-minute) is real and held on real torajs substrate work, not a synthetic micro-bench. correctness-equivalence proven (conformance under iter tr = 629/0/1). bench+ship keep `--release` | hold ≤ 5 s typical; track | ≤ 2 s `[D]` |
| sccache hit rate | **structurally ~0 % for torajs inner loop** `[M]` — global shared server, bin/changed-src non-cacheable by design (NOT a fixable misconfig) | n/a — dropped as a torajs lever (was a misconception); deps-only cold-start benefit is incidental | n/a |
| no-op rebuild | **0.05 s** `[M]` (cargo correctly skips; steady-state optimal) | unchanged | unchanged |

## 2. cleanup — garbage / stale-artifact control

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| junk-source coverage | **devperf-#1 byproduct `target/iter` (195 MB) now covered** `[M]` ✅ — was a real gap (the fast-iter cache the cleaner couldn't see); `target/release` stays guarded (bench-required), verified in dry-run | each new junk source gets a grep-able rule (process invariant) | n/a (coverage, not size, is the metric) |
| cleanup invocation | **dry-run-default; `--force` operator-invoked under disk pressure (by design, not a gap)** `[M]` — dry-run correctness verified (right targets, guards hold). Auto-running `--force` to tick a metric would delete useful cache for no reason — contradicts the pillar's own "reclaim under pressure" philosophy | hook-triggered at session boundaries (Claude Code settings.json / update-config skill — deferred per README) | self-auditing: warn when an *unknown* large dir appears `[D]` |
| disk-hygiene incidents | history: 1 catastrophic (688 GB bun-build, pre-hardev) `[M]` | 0 (hook-enforced) `[D]` | 0 + early-warning before threshold `[D]` |

## 3. taskq — L1–L4 governance

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| invariants spec | **7 machine-checkable invariants spec'd** `[M]` ✅ taskq v0.1.9 — `hardev/taskq/README.md` INV-1…7, each grounded in a concrete observed drift (was: nothing, governance was vibes) | a checker enforces them exit-coded | checked on every memory edit |
| observed drift | **5 incidents catalogued + 4 de-drifted in the live plan source** `[M]` ✅ — D1 header self-contradiction / D2 L4-vs-directive "P7.4 95% vs CLOSED" / D3 L3a stale P7 hot prose / D4 dead `#15→#12` pointer fixed & banner-marked archaeology this commit (taskq INV-1/2/3/5 first application) | drift checker auto-flags | 0 silent drift |
| L4 trigger form | **INV-6 spec'd: must be a state predicate, never "feel like it"** `[M]` (enforcement = the future checker) | checker rejects non-predicate triggers | auto-checked after every ship |

## 4. bench — performance · coverage · reporting

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| regression verdict | **`bench compare` — machine verdict + exit code, reproducible, in-repo** `[M]` ✅ B1 SHIPPED `0726979`+ (was: ad-hoc agent python). artifact_bytes hard gate + noise-aware run_ms; reproduced the prior hand-finding exactly | (met early) add N-run aggregation input (B1b) | statistical (MAD/Mann-Whitney) noise-band verdict in CI `[D]` |
| trustworthy signal | **artifact_bytes only** `[M]` (deterministic); run_ms cross-day **±15 % systematic + up to +200 % single-point** `[M]` = unusable for verdicts | artifact-gate primary + same-machine-state run_ms `[D]` | machine-hygiene-gated run_ms becomes trustworthy too `[D]` |
| per-commit gate wall | **seconds for tr-unchanged** `[M]` ✅ B2+B2b SHIPPED — `--self` drops 6 foreign runtimes (~¼); `--vs <baseline>` artifact-precheck skips ALL timed runs when artifacts byte-identical (measured **1.91 s** vs ~10 min full), full timed fallback when any differ (coverage preserved). was ~10 min full 8-runner | (met) | full cross-runtime only at phase-close `[D]` |
| coverage vs phase under dev | **gap** `[M]`: 26 cases, **no bigint, 1 exception case** — P7 (Error/bigint) shipped with no direct bench coverage | each phase adds ≥1 hot-path case for its substrate `[D]` | coverage auto-tracks the active phase `[D]` |
| multi-run aggregation | **`bench run --runs N` — native interleaved N-pass median + MAD in one json** `[M]` ✅ B1b SHIPPED (was: same-name overwrite, agent log-parsing) | (met) feed aggregated json into a statistical noise-band verdict | built into a CI statistical `bench compare` `[D]` |

## 5. test262 — ECMAScript spec conformance

The tc39/test262 reference suite is the cross-engine standard every TS/JS
runtime is measured against. Distinct from §1's *subset* conformance
(631/0/1 — torajs's hand-curated in-tree fixtures that prove the strict
TS slice it accepts is bun-equivalent). test262 measures **how much of
the full ECMAScript surface torajs has reached so far** — the leading
indicator for "what % of programs Bun runs will torajs also run".

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| in-scope pass rate | **3344/28314 (11.81 %)** `[M]` `hardev/test262-latest.json` @ `2486552` (53,174 total cases, 786 s wall, 8 workers; in-scope = bun-pass cases torajs at least attempted = 28,314) | ≥ 30 % `[D]` (each P8–P13 substrate phase opens spec slices: classes, regex, async, Unicode, IEEE-754) | **≥ 90 %** on the in-scope slice — v1.0 hard gate (`docs/100-percent-plan.md`) `[D]` |
| tr-accepted parity | **3344/3615 (92.50 %)** `[M]` — when tr accepts a case, the result agrees with bun 92.5 % of the time; the remaining 271 are "bug" classified (real spec gaps to fix, not subset-boundary rejects) | ≥ 99 % (zero silent divergence on the accepted slice) `[D]` | 100 % (every accepted program is byte-equivalent to bun) `[D]` |
| total bug count | **271** `[M]` (cases tr accepts but produces wrong output) | ≤ 100 (P8 + P11 + P12 each close known bug clusters) `[D]` | ≤ 10 (residual rare-edge spec corners) `[D]` |
| dashboard surface | **devserver :6002 `Test262Card` — pass/in-scope + rate + breakdown (bug / incompatible / bun-skip) + ran-at + HEAD stamp** `[M]` ✅ DASH-T262 SHIPPED — single source of truth so any takagi/agent glance sees the live number | unchanged | dashboard auto-refresh via per-commit `torajs-test262 --json` in CI `[D]` |
| measurement freshness | **manual `--json` invocation** `[M]` (8 workers · ~13 min wall · zero-dep runner) — re-run after substrate phase ships | nightly cron (CI machine) | per-PR delta in CI `[D]` |

How the number is *meant* to be read:

- `pass / inScope` is the **headline** — what spec slice torajs gets right when it doesn't bail out at the subset boundary.
- `incompatible` (24,699) is **not** a bug — those are cases tr correctly rejects with a documented boundary message (regex, Symbol, Proxy, Function constructor, ...). Every one of them points at a future roadmap phase, not a permanent design boundary (`feedback_torajs_ambition`).
- `bun-skip` (24,860) are negative tests, harness-dependent tests, and test262 internals that the oracle (bun) also doesn't run — not interesting for either runtime.
- `bug` (271) is the real backlog: cases that pass the subset boundary, get accepted by tr, but diverge from bun. **These are the spec gaps** to close phase by phase.

## 6. autorun — agent-session 编排治理 (incubating, v0.1.x)

Keep long-running agent autorun **low-drift, observable, machine-
governed** instead of hand-rotated. Methodology unchanged — **measure
first, automate second**. v0.1.x intentionally ships no daemon and no
auto-rotation; it ships the protocol + the structured log + the metric
slots, so a 1-week baseline can ground the P1 automation decisions.

| Metric | now (v0.1.x) | after v1 | after v2 |
|---|---|---|---|
| rotations recorded | **52 / week effective** `[M]` — 10 rows over 31.9 h (1.33 d) in `hardev/autorun/rotations.jsonl` @2026-05-21; cadence 5× spec floor (≥10/wk); all 10 `trigger=self` (agent self-eval rate = 100 %) | ≥ 10 rotations / week (steady cadence visible) — **met** | dashboard panel surfaces 7-day rolling rate |
| session length (commit→rotation interval) | **median 125 min, mean 213 min, range 40–779 min** `[M]` — 9 gaps computed from 10 rows in `hardev/autorun/rotations.jsonl` @2026-05-21; the 2 long gaps (327 min, 779 min) are overnight, not drift | distribution stable to ±25 % (cadence predictable, not drift-driven) | dashboard surfaces median + p95 |
| handoff fidelity | **`[D]` takagi hand-flagged** — % of post-rotation sessions where the first user message does NOT need to clarify lost state | ≥ 95 % | ≥ 99 %, auto-detected by comparing handoff vs first-turn outputs |
| drift-incident rate | **`[D]` takagi hand-counted** — events per session where Claude broke a CLAUDE.md HARD RULE (Chinese-only / 4-layer / disk hygiene) | ↓ trend after rotation cadence stabilises | ≤ 1 / 10 sessions, auto-detected pre-rotation |
| unstaged-loss incidents during rotation | **0** `[M]` — 10 manual rotations 2026-05-19..21 with 0 incidents takagi-flagged; P0 has no automated /clear so risk is currently zero by construction | 0 (INV-2 enforced by P1 watcher pre-act gate) | 0 + automatic rollback if regression detected |
| conformance regression introduced by rotation | **0** `[M]` — 10 rotations 2026-05-19..21, `conformanceBefore` 631 → 650 monotonic across the series (rotations.jsonl); 0 incidents takagi-flagged | 0 (INV-3 enforced — post-rotation conformance ≥ pre-rotation) | 0 + post-rotation gate runs automatically |
| protocol surface | **CLAUDE.md HARD RULE «Autorun rotation protocol» + `hardev/autorun/README.md` (P0 SHIPPED)** `[M]` — sequence: `/handoff:handoff save` → `hardev/autorun/trigger.sh self` → no further tokens this turn | unchanged at v1 | unchanged |
| automation level | **manual** `[M]` — operator runs `/clear` and `/handoff:handoff resume` themselves; the agent only records intent | **automatic** `[D]` — Stop hook writes marker, watcher (`launchd`) drives `/clear` + resume via `tmux send-keys` | self-healing — daemon heartbeats and crash-restarts itself, multi-project |

How to read this section: every `[D]` here is **intentional** — the
data needed to ground a real `now` value is what the v0.1.x ship is
designed to gather. Filling them in is a normal v1 deliverable, NOT a
debt. The headline judgement of this pillar is precisely that "ship
mechanism without metric" is the wrong order; the slot stays empty
until the trigger log has enough rows to compute one.

### Baseline observation @2026-05-21 (10-row review per README §"后续路径")

The first review window closed at `rotations.jsonl` row 10. Numbers in
the *now* column above derive from this window; this sub-section keeps
the qualitative observations the table cannot hold:

1. **Cadence beats spec floor 5×.** 10 self-triggered rotations in
   31.9 h (1.33 d) is `52 / week` extrapolated — well past the README
   floor of `≥ 10 / wk`. The pillar is **not** waiting on cadence to
   justify P1 automation; cadence is established.

2. **Conformance trajectory is monotonic non-decreasing across the
   window.** `conformanceBefore` goes `631 → 631 → 632 → 634 → 637 →
   640 → 640 → 644 → 646 → 650`. The two flats (#1→#2 substrate-only
   commit; #6→#7 dashboard-only commit) are explainable. P0 dogfood
   itself introduced **0** conformance regressions — same as the *now*
   number, with the underlying series as `[M]` provenance.

3. **handoffAgeSec outlier — the canonical P1 case.** Row #6
   (`r-1779265047-549c`, prevHead `e1f8219`) records
   `handoffAgeSec = 7489` (≈2 h 5 min), an order of magnitude beyond
   the planned `INV-1 < 90 s` boundary. Other 9 rows are ≤ 7 s. Reading
   the row: `handoffSha` matches row #5's, but `prevHead` advanced —
   i.e. `handoff.md` was saved 2 h before `trigger.sh` ran, and commits
   were made in the interval. The handoff describing what shipped is
   now strictly out of date relative to the trigger's `prevHead`. **In
   P0 this fails silently and nobody notices.** This single row is the
   strongest existing justification for P1 automation: the watcher
   gate must refuse to write `.claude/autorun-marker` while
   `handoff.md` is older than the configured threshold, forcing the
   agent to re-save before the rotation can proceed.

4. **Trigger source split is degenerate.** 10 / 10 rows are
   `trigger = self`; zero `manual`. takagi has not had to step in
   once. Good signal for P1 — the agent's self-eval is already the
   working hand on the wheel, automation can ride on top without
   changing who decides "now".

5. **What remains `[D]` after this window.** Handoff fidelity and
   drift-incident rate stay `[D]` — they need first-turn behavioural
   logging the P0 ship does not yet collect. P1 watcher is the right
   layer to start counting these (e.g. heuristic on the first user
   message after resume), so the slots become deliverables in v1
   alongside the automation itself, not blockers ahead of it.

The window closes the **measure-first** half of v0.1.x. Next is
mechanism: P1 (`Stop` hook + watcher + `INV-1..5 check.sh` + launchd).
See `hardev/autorun/README.md` §"Architecture" Layer 1 and Layer 3.

## How to keep this file honest

1. Never write an untagged number. `[M]` must cite the command/log.
2. When a pillar item ships, re-measure its metric, update the *now*
   column, and record the wall in `CHANGELOG.md`.
3. A metric that moves the wrong way is a regression — treat it like a
   failing test, not a footnote.
4. The headline `[A]` flags (sccache 0 %) are P0 investigations, not
   trivia — establishing them *is* the deliverable of v0.1.0.
