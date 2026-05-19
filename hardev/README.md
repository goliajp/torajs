# hardev — Rust-specialized R&D-support framework

> **What**: `hardev` is a support framework for **large-scale, high-quality,
> high-performance, complex-systems R&D in Rust**. It makes the develop →
> verify → ship loop fast, clean, trustworthy, and disciplined **without ever
> trading away verification coverage or correctness**.
>
> **Status**: incubating **in-project, inside `torajs`** (the company's #1
> core project, a bun-class AOT TypeScript runtime). torajs's scale —
> too large, too important, too complex, too hard to drive without
> advanced tooling — is exactly the proving ground hardev needs. It is
> deliberately grown here first; extraction into a standalone tool comes
> only when the pillars are concrete and battle-tested. **Rust-specialized**:
> cargo / sccache / cargo-target / rustfmt / clippy / hyperfine-aware by
> design, not a generic CI shell.
>
> **Version**: see `VERSION` + `CHANGELOG.md` (authoritative). All four
> pillars now have shipped tooling/spec; a live dashboard
> (`web/`, `bun run dev`) surfaces progress+bench. taskq's own
> `check.sh` enforces this file's freshness — so this paragraph
> intentionally does **not** hard-code a version number (that would be
> the very drift taskq exists to kill); read `VERSION`.

## First hard rule (non-negotiable)

> **No optimization or automation may reduce verification coverage or
> correctness.**

Concretely: never drop a conformance case, never skip the bun-oracle
cross-check, never bypass the `tr build` AOT path, never relax the
zero-warn / zero-fail / fmt-clean ship gate. hardev may only change
**wall-clock / disk / human cost** — never **what is verified**. Any
"looks faster but verifies a bit less" approach is rejected outright
(aligned with `.claude/rules/torajs-design-principles.md` 规范 pillar and
`feedback_no_tech_debt`). Every hardev item must carry a
**machine-decidable acceptance** (e.g. "conformance still 629/0/1 AND
wall < X"), not a vibe.

## The four pillars (initial focus)

hardev v0.x is scoped to the four areas torajs most needs support in.
Each pillar has an owner artifact today and a backlog of concrete,
leverage-ranked, acceptance-gated work.

| Pillar | Scope | Today's artifacts |
|---|---|---|
| **1. devperf** — dev-loop performance | build/cache speed, sccache, project-private cargo-target, the real levers (not folklore) | SHIPPED: sccache myth root-caused + `[profile.iter]` (edit→rebuild 28.5s→2.49s). `environment.md` ground truth; `optimization-backlog.md` |
| **2. cleanup** — garbage / stale-artifact control | enumerable regenerable junk reclaimed safely; dry-run-default; never touch source/committed/foreign | SHIPPED: `cleanup/clean.sh` (covers target/iter; target/release guarded) |
| **3. taskq** — task-queue L1–L4 governance | making the 4-layer planning architecture an enforced, machine-checkable discipline rather than prose | SHIPPED: `taskq/README.md` INV-1…7 spec + `taskq/check.sh` enforcing INV-1a/1b/5 exit-coded |
| **4. bench** — benchmark performance · coverage · reporting | trustworthy, reproducible, machine-judged regression verdicts; fast per-commit path; coverage that tracks the phase under development | SHIPPED: B0/B1/B1b/B2/B2b (`bench compare`, `--self`, `--vs` precheck). `optimization-backlog.md` §bench; `environment.md` §4b |
| **+ web** — live dashboard (reporting surface) | progress + bench over a real-data snapshot, pitch.html aesthetic, no GDS | SHIPPED: `web/` (`bun run dev`) |

### Pillar 3 — taskq L1–L4 governance (charter)

torajs already runs on a strict 4-layer information architecture
(CLAUDE.md "Planning Architecture HARD RULE"): **L1 roadmap / L2 version
boundary / L3a hot plan / L3b cold backlog / L4 trigger**. Today it is
maintained by hand in a status-memory file and enforced only by
discipline. The taskq pillar's mandate: turn that into a **checkable,
low-drift discipline** — layer-tagged work items, a trigger predicate
that is machine-evaluable, and "after every ship: re-check L4" as an
automatable step — so the governance survives long autonomous sessions
without the classic failure (hot plan polluted by cold noise, triggers
written as "feel like it", decisions pushed back to the human). v0.1.0
records the charter; concrete tooling is backlog.

## Index

| Path | Role | Evolution |
|---|---|---|
| `README.md` | this charter — identity, first hard rule, four pillars | stable; bump on pillar/scope change |
| `VERSION` / `CHANGELOG.md` | version record (semver-ish; incubation) | one entry per shipped hardev change |
| `environment.md` | build/cache/bench **ground truth** + corrected misconceptions (devperf + bench pillars) | update whenever an environment fact changes |
| `optimization-backlog.md` | leverage-ranked, acceptance-gated, quality-neutral backlog (devperf + bench) | mark done + record measured wall; append new items |
| `cleanup/clean.sh` | safe dry-run-default stale-file cleaner (cleanup pillar) | add a grep-able glob rule per new enumerable junk source |
| `taskq/README.md` + `taskq/check.sh` | L1–L4 governance — 7 invariants (INV-1…7); `check.sh` enforces INV-1a/1b/5 exit-coded | extend checker to INV-2/3/4/6/7 |
| `web/` | live dashboard webserver (Vite/React, no GDS, pitch.html design) — dev progress + benchmark over a real-data snapshot | re-run `scripts/snapshot.mjs` to refresh; `bun run dev` to serve |

## How to extend hardev (for future sessions / developers)

1. **New optimization/automation idea** → first self-check the first hard
   rule (does it change wall-clock or *what is verified*? changing the
   latter = rejected). If it passes, insert into `optimization-backlog.md`
   by estimated leverage with a **machine-decidable acceptance**.
2. **New environment fact / gotcha** → record in `environment.md`,
   especially counter-intuitive ones (hardev's origin was one: an
   external disk is *slower* for cargo's small-file random IO than the
   internal disk; sccache is the real build lever).
3. **New junk source** → add a grep-able glob + verify-before-delete rule
   to `cleanup/clean.sh`; keep dry-run the default; never touch
   source/committed/foreign/non-self artifacts.
4. **New pillar tooling** → grow it under a pillar; only create a
   subdirectory once there is something concrete (don't pre-scaffold
   empty trees — incubation discipline).
5. **Automation triggers** (the "auto" in auto-cleanup) → belong to Claude
   Code hooks (settings.json) via the `update-config` skill; the script
   bodies live here, the hook only schedules them. Make the script
   correct & safe first, automate second.

## Relationship to existing rules

- hardev does **not** replace `.claude/rules/` (those are the HARD RULES +
  pipeline discipline). hardev is the **tooling & plan layer** for
  executing that discipline faster and more reliably.
- `cleanup/clean.sh` is the tooled form of CLAUDE.md's Disk Hygiene HARD
  RULE, not a parallel regime.
- Cache/build conclusions stay aligned with
  `~/.claude-shared/global/cargo-target-dir.md`; torajs's exception
  (project-private internal target) is recorded in `environment.md`.
