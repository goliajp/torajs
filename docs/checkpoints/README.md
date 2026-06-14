# torajs project checkpoints

> Periodic 4-axis ground-truth snapshots of torajs project state. Each
> checkpoint is a self-contained MD with the same 4 sections (整体 /
> metal 化自研 / test262 / benchmark) so cross-checkpoint diffs are
> mechanical. Trigger conditions, template, and trend table below.

## Trigger conditions

A checkpoint is produced when ANY of the following fires:

1. **roadmap phase transition** — a `## Pn — ...` section in
   `../roadmap.md` moves from CURRENT → DONE (e.g. P9 → P10).
2. **substrate trunk close** — a multi-chunk trunk in
   `../../.claude/plan-state.md` L3a finishes all its chunks (e.g.
   W-J trunk A0→D close, Phase 2 fn-name trunk close).
3. **roadmap framing change** — takagi reframes a phase scope / order
   / acceptance criterion (the roadmap rewrite or insertion of new
   axis).
4. **takagi explicit ask** — "做个 checkpoint" / "全面汇报" / `/checkpoint`.

Mechanical cadence (commit count) is **not** a trigger — substrate
shape matters, not commit volume.

## Template

Each checkpoint MUST have these 4 fixed top-level sections, in this
order, with the same data shape so the trend table can stack columns:

1. **整体进度 / 当前位置** — HEAD short hash, branch, roadmap phase
   (CURRENT), active L3a hot trunk (chunk N of M), this-period ship
   summary (N commits), 5-line `git log --oneline -5`.
2. **Metal 化 / 全自研** — workspace crate count, metal-core ext dep
   count (target 0), grandfathered ext dep list (cli + cloud-api
   only), C runtime status (`find crates -name '*.c'` count).
3. **test262** — in-scope passRate (`passTotal / 53174`), top
   blocking bucket + size, next-trunk plan reference, in-house
   conformance gate baseline (`N / F / S`).
4. **Benchmark** — bench JSON path + git sha + host + timestamp,
   representative 10-15 case TR vs BUN-AOT run_ms table, artifact
   size (tr vs bun-aot vs rust), known regression follow-up list.

Every number must be sourced (file:line, command output, or jq query)
— no recall, no estimate, per anti-hallucination HARD RULE.

## File naming

```
docs/checkpoints/<YYYY-MM-DD>-<trigger-tag>.md
```

`<trigger-tag>` examples: `p9-close` / `w-j-a3a` / `framing-rewrite-v6`.
Use the substrate trunk / phase short-name when possible so chronological
sort matches semantic order.

## Trend table

Stacked-column metric diffs across checkpoints — every checkpoint MUST
append one row. Empty cells are OK when data is unavailable at that
point. Bench column = TR vs BUN-AOT geomean over the same
representative case set (or note the changed set in commit msg).

| date | HEAD | roadmap phase | conformance gate | test262 in-scope % | bench geomean tr/bun-aot | tr artifact (MB) | workspace crates | metal-core ext dep | total LOC |
|------|------|---------------|------------------|--------------------|---|---:|---:|---:|---:|
| 2026-06-14 | `a53a11de` (W-J A3a, +0e100f63 dashboard) | P10 (Promise/async/Generator) hot · W-J substrate trunk 4/9 chunk | 825/0/4 | 4.17% (2217/53174); 含 oracle 12.66% (6730) | tr ~2× faster (closure/throw/promise/startup/fib/array hot 9 case) · 持平 mandelbrot/gcd · slower popcount/json-stringify | 1.45 | 44 | 0 (cli + playground-api 隔离 scope) | 236,190 |

## How to add a checkpoint

1. Copy the template structure from the most recent checkpoint file.
2. Run all the source-fetching commands fresh — never copy numbers
   from a prior checkpoint.
3. Add one row to the trend table above with same-column ordering.
4. Commit with `chore(checkpoints): <YYYY-MM-DD> <trigger-tag>` form.

## Cross-checkpoint diff convention

When a follow-up checkpoint references a prior one, use:

- `Δ vs 2026-06-14-w-j-a3a.md`: `+418 test262 pass` / `-3.4 MB tr binary` / `+2 crates` / etc.

Diffs go in the new checkpoint's section narrative (under each of the
4 sections), not in the trend table — the table stays raw values.
