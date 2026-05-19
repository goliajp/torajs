# hardev changelog

Incubation versioning, semver-ish. One entry per shipped hardev change.
A pillar item is "shipped" only when its metric in `metrics.md` is
re-measured and the *now* column updated.

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
