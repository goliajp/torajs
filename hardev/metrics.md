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
  629/0/1 still proves correctness, coverage unchanged), bench+ship
  keep fat-LTO release. This is *exactly* why metrics precede
  optimization: a global tool's global snapshot had been written into
  ground truth as a torajs-specific conclusion, hiding the true lever.

## 1. devperf — dev-loop performance

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| full conformance wall (629 cases) | **~3.0–3.5 min** `[M]` parallel 8-worker (174–208 s ×N this session; shipped `6ab22f9`, was ~30 min serial) | ≤ 2 min (artifact-precheck skips timed re-verify when tr unchanged) `[D]` | ≤ 30 s for the common "tr unchanged" case `[D]` |
| **edit→rebuild `tr` wall** (THE inner-loop metric) | **2.49 s** `[M]` ✅ devperf #1 SHIPPED `<this commit>` — `[profile.iter]` (lto=off/cgu=256/opt=1) for functional+conformance; was 28.5 s under `--release`; **~11.4×**. correctness-equivalence empirically proven (full conformance under iter tr = **629/0/1**, opt-level/LTO are semantics-invariant). bench+ship keep `--release` (separate `target/release/tr`) | (met early) hold ≤ 5 s; track regressions | ≤ 2 s `[D]` |
| sccache hit rate | **structurally ~0 % for torajs inner loop** `[M]` — global shared server, bin/changed-src non-cacheable by design (NOT a fixable misconfig) | n/a — dropped as a torajs lever (was a misconception); deps-only cold-start benefit is incidental | n/a |
| no-op rebuild | **0.05 s** `[M]` (cargo correctly skips; steady-state optimal) | unchanged | unchanged |

## 2. cleanup — garbage / stale-artifact control

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| reclaimable junk visible to tool | **~0 MB right now** `[M]` (dry-run; tree kept clean by manual hygiene this session) — but tool only knows the globs it has | every enumerable junk source has a grep-able rule `[D]` | n/a (coverage, not size, is the metric) |
| cleanup invocation | **manual, dry-run-default, never run `--force` for real** `[M]` (smoke-tested only) | invoked automatically via Claude Code hook at session boundaries `[D]` | self-auditing: warns when an *unknown* large dir appears `[D]` |
| disk-hygiene incidents | history: 1 catastrophic (688 GB bun-build, pre-hardev) `[M]` | 0 (hook-enforced) `[D]` | 0 + early-warning before threshold `[D]` |

## 3. taskq — L1–L4 governance

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| plan representation | **hand-maintained prose** in status-memory `[M]` | layer-tagged structured items (L1/L2/L3a/L3b/L4 explicit fields) `[D]` | same, tool-validated on every edit `[D]` |
| observed drift this session | **3 incidents** `[M]`: stale "2/5 done" predated P7.3; L2 directive changed twice mid-session without memory catching up until prompted; tasks #16–#20 filed ad-hoc | drift detector flags layer-mixing / stale counters `[D]` | 0 silent drift (machine-checked invariants) `[D]` |
| L4 trigger form | **prose, sometimes "feel like it"** `[M]` | machine-evaluable predicate per phase `[D]` | auto-checked after every ship; blocks wrong advance `[D]` |

## 4. bench — performance · coverage · reporting

| Metric | now (v0.1.0) | after v1 | after v2 |
|---|---|---|---|
| regression verdict | **manual ad-hoc python by the agent** `[M]` — not a command, not reproducible, not in-repo | `bench compare` subcommand: machine verdict + exit code `[D]` | statistical (MAD/Mann-Whitney) noise-band verdict in CI `[D]` |
| trustworthy signal | **artifact_bytes only** `[M]` (deterministic); run_ms cross-day **±15 % systematic + up to +200 % single-point** `[M]` = unusable for verdicts | artifact-gate primary + same-machine-state run_ms `[D]` | machine-hygiene-gated run_ms becomes trustworthy too `[D]` |
| per-commit gate wall | **~10 min** `[M]` full 8-runner (585 s clean / 912 s under load this session) | **seconds** for tr-unchanged via `--self` + artifact-precheck `[D]` | seconds always; full cross-runtime only at phase-close `[D]` |
| coverage vs phase under dev | **gap** `[M]`: 26 cases, **no bigint, 1 exception case** — P7 (Error/bigint) shipped with no direct bench coverage | each phase adds ≥1 hot-path case for its substrate `[D]` | coverage auto-tracks the active phase `[D]` |
| multi-run aggregation | **not supported** `[M]` (same-name json overwrite; agent parses logs) | native N-run median/MAD artifact `[D]` | built into `bench compare` `[D]` |

## How to keep this file honest

1. Never write an untagged number. `[M]` must cite the command/log.
2. When a pillar item ships, re-measure its metric, update the *now*
   column, and record the wall in `CHANGELOG.md`.
3. A metric that moves the wrong way is a regression — treat it like a
   failing test, not a footnote.
4. The headline `[A]` flags (sccache 0 %) are P0 investigations, not
   trivia — establishing them *is* the deliverable of v0.1.0.
