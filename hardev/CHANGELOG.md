# hardev changelog

Incubation versioning, semver-ish. One entry per shipped hardev change.
A pillar item is "shipped" only when its metric in `metrics.md` is
re-measured and the *now* column updated.

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
