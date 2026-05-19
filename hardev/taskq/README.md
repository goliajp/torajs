# hardev taskq pillar — L1–L4 governance

> **Mandate**: make the project's 4-layer planning architecture
> (CLAUDE.md "Planning Architecture HARD RULE": **L1 roadmap / L2
> version / L3a hot / L3b cold / L4 trigger**) a **low-drift,
> machine-checkable discipline** instead of hand-maintained prose that
> silently accretes and contradicts itself.
>
> Methodology, same as every hardev pillar: **spec before tooling**.
> v0.1.9 ships the invariants spec + its first application (de-drift
> the live plan source). A later increment builds the automatic
> checker; this doc is what that checker must enforce.

## Why this pillar exists — drift is real and demonstrable

The plan source is `memory/project_status_<date>.md`. Its own usage
protocol (frontmatter): *"开会话/loop fire 先读 L3a 顶部 take 一项"*. So
stale content there directly misroutes the next session. Concrete
drift observed **2026-05-19, in the live file, while grounding this
pillar** (not hypothetical):

- **D1** — the top one-line header still said `HEAD 683bd95 … closing
  629-gate in-flight` long after HEAD had moved 10 commits on and the
  gate had finished.
- **D2** — the **L4 checklist section said `P7.4 ~95% (仅剩 frozen
  收尾); L3a next = P7.4 收尾`** while the authoritative directive
  block at the top said **`P7.4 CLOSED, autorun on P7 paused`**. Two
  sections of the same file, flatly contradicting, on the load-bearing
  "what's next" question.
- **D3** — the entire `## L3a — Hot 计划` section was still the P7.2 /
  P7.3 / P7.4 hot queue, all of it shipped; the *actual* hot work
  (hardev pillars) appeared only in the directive block.
- **D4** — `L3a 顺位：#15 prefix → #12 a-b 收尾` pointed at two tasks
  both already completed.
- **D5** — earlier in the same session: a stale `2/5 done` L4 counter
  that predated P7.3; the L2 directive changed twice with the memory
  lagging until prompted.

A reader following the file's own protocol (`read L3a top, take #1`)
would have picked up the **stale shipped P7 plan**, not the live
hardev focus. That is the governance failure, with receipts.

## The invariants (what a drift-checker must enforce)

Each is a machine-decidable predicate over the plan-source file.

| # | Invariant | Drift it would have caught |
|---|---|---|
| **INV-1 single source of truth** | Exactly one block is authoritative for "current state / HEAD / what's next". The one-line header and the authoritative block must not assert different HEADs or phase states. | D1 |
| **INV-2 layer agreement** | L4-checklist phase status, the directive block's phase status, and L2 must agree (no section says a phase is open that another says is closed). | D2 |
| **INV-3 hot = actual** | The `L3a — Hot 计划` top item must equal the work actually in progress (cross-check vs git HEAD subject / task list in_progress). A hot section full of shipped items is a violation. | D3 |
| **INV-4 pointers resolve forward** | Every `next = X` / `L3a 顺位` / `#N` pointer must reference work that is NOT yet done (cross-check vs commit log + task statuses). | D4 |
| **INV-5 closed work is archaeology, not hot** | A shipped/closed phase's hot-plan prose must be moved under an explicit `archaeology` / `closed` heading, never left inline as if live. | D3, D5 |
| **INV-6 L4 trigger is a predicate** | The L4 trigger must be a machine-evaluable state predicate (`X done` / `metric ≥ Y`), never "feel like it" / "等做完再说". | D5 (stale counter = predicate not re-evaluated) |
| **INV-7 counters re-derived, not typed** | Any `N/M done` style counter must be derivable from the checklist items, not a hand-typed number that can lag. | D5 (`2/5 done`) |

## v0.x roadmap for this pillar

- **v0.1.9 (this)**: this spec + **first application** — de-drift the
  live plan source (collapse the stale P7 L3a/L4 prose under
  archaeology, make every section agree with the directive block).
  Pure governance; no language/runtime change.
- **next**: a `hardev` checker (script or `bench`-sibling subcommand)
  that parses the plan-source file and asserts INV-1…7, exit-coded,
  runnable as a pre-commit / session-boundary gate (analogous to
  `bench compare` for perf). Spec → tooling, same as every pillar.
- **v2**: invariants checked on every memory edit (hook), counters
  auto-derived, zero silent drift.

## First hard rule still applies

taskq tooling may only make the governance *checkable / lower-drift*.
It must never *relax* a planning HARD RULE (no "auto-advance past a
trigger", no hiding a contradiction by deleting one side — a
contradiction is resolved by making the stale side correct, never by
silencing the check).
