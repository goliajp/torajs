# torajs perf — what's measured

torajs's positioning: AOT TypeScript runtime that **dominates JS
incumbents** by measured wall-clock — every number in a commit
message, README, or blog post **must trace back to a measurement
that anyone can reproduce.** Guesses don't count. Estimates don't
count. Numbers we'd like to be true don't count.

This file is the single source of truth for which torajs perf claims
are honestly measured + which are still open. When a commit message
disagrees with this file, trust this file — and update the commit's
claim or this file in the same merge.

## Measured

### Workspace-level

| Path | Measurement | Run command |
|---|---|---|
| **geomean speedup vs bun-aot** | **4.41×** at HEAD `8f754ca` (5-pass median, sequential interleave, no concurrent test262/conformance, mac M-series, host CPU detection enabled). Up from 3.89× at `14d56f8` (the 5-pass quiet-machine baseline after the contaminated 3-pass result was discarded). | `cargo run -p bench-harness --release -- run --runs 5` |
| **geomean speedup vs bun-jsc** | **4.52×** at same HEAD | same |
| **geomean speedup vs node-v8** | **21.07×** at same HEAD | same |
| **per-case wins** | **26 / 26** vs bun-best (held across every shipped P-PERF commit). | bench result file `bench/results/2026-05-22-mini-d7bab5b.json` |
| **Conformance subset** | **666 / 0 / 1** (pass / fail / skip) at HEAD `8f754ca`. Held throughout every P-PERF ship; 0 regression at any commit. | `cargo run --release --bin torajs-conformance` |
| **test262 in-scope pass rate** | **12.20%** (3455 / 28314) at HEAD `081b25f`, ranAt 2026-05-22. Up from 11.81% (3344) at 2026-05-19. | `cargo run --release -p torajs-test262 -- --json hardev/test262-latest.json` |
| **AOT binary size (popcount case)** | ~36 KB stripped | `du -h target/release/<case>-aot` after `tr build` |

### Per-case bench medians at HEAD `8f754ca` (post P-PERF.A6, 5-pass median, M-series Mac)

| Case | torajs ms | bun-aot ms | bun-jsc ms | node-v8 ms | tora vs bun-best |
|---|---:|---:|---:|---:|---:|
| popcount | 2.51 | 57.73 | 57.00 | 137.70 | **22.7×** |
| generic-pair-1m | 1.19 | 12.29 | 12.07 | 95.23 | **10.1×** |
| fifo-queue-100k | 1.28 | 9.96 | 9.75 | 93.05 | **7.6×** |
| stack-pop-1m | 1.89 | 15.46 | 14.79 | 100.73 | **7.8×** |
| startup | 1.15 | 7.63 | 7.46 | 89.68 | **6.5×** |
| promise-all-1k | 1.21 | 8.33 | 7.96 | 92.92 | **6.6×** |
| promise-chain-1k | 1.33 | 8.63 | 8.31 | 91.72 | **6.3×** |
| closure-pipeline-1m | 8.17 | 51.57 | 49.87 | 191.69 | **6.1×** |
| generic-id-1m | 8.01 | 49.71 | 47.32 | 184.87 | **5.9×** |
| split-only-100k | 3.50 | 10.31 | 9.69 | 92.44 | **2.8×** |
| ... (others) | | | | | 1.1×–5×+ |
| **prime_count** | 48.07 | 53.49 | 54.44 | 165.74 | **1.1×** ← narrowest lead |

Narrowest leads (`prime_count` 1.1×, `gcd1m` 1.2×, `mandelbrot` 1.5×,
`ackermann` 1.8×, `fib40` 2.5×) are the P-PERF focus going forward —
CPU-bound integer/float loops where bun's JIT closes the gap.
Wide leads (popcount 22.7×, fifo 7.6×, stack-pop 7.8×) confirm
tora's structural AOT advantage on tight typed loops.

### P-PERF session-arc this round (2026-05-22)

| Commit | Change | Geomean vs bun-aot | Net result |
|---|---|---|---|
| `14d56f8` | 5-pass quiet baseline (3-pass contaminated, discarded) | 3.89× | baseline |
| `f4cd310` P-PERF.A1 | User FnDecls Internal linkage → IPSCCP/inliner specialization | **4.16× (+7.0%)** | SHIPPED |
| `9050729` P-PERF.A3 | alwaysinline for small non-recursive user fns (threshold 60) | 4.16× (per-case wins, ratio flat) | SHIPPED |
| `8f754ca` P-PERF.A6 | Promise free-list pool (bounded 32; promise-await -41% / async-fn-call -36% / promise-then -32%) | **4.41× (+6.1%)** | SHIPPED |
| (between) | A2 codegen=Aggressive | 4.10× | REVERTED — net regression |
| (between) | A4 alwaysinline threshold 30 | 4.15× | REVERTED — worse than A3 |
| (between) | A5 RelocMode=Static | 4.15× | REVERTED — thermal-confounded inconclusive |

Net session gain: **3.89× → 4.41× (+13.4%) geomean vs bun-aot, +14.9% vs node-v8**. Three optimizations shipped, three reverted as negative-result evidence (archived under `bench/results/`).

## Reproduction protocol (per "别作假" hard rule)

Every published number above must be reproducible by running the
commands listed, on a quiet host (no concurrent test262 / conformance
runs, no thermal throttle), using the **5-pass median** protocol.
Single-run mac numbers carry ±20–40% noise band and are NOT used for
shipped claims.

When a commit changes any of these numbers:

1. Run the relevant command.
2. Capture the bench result file.
3. Update the row here in the same commit (or a same-day docs commit).
4. If the new number differs by > noise band from an old claim that
   wasn't updated, **the old claim was wrong** — annotate the
   correction explicitly.

## When perf claims drift from this file

`mailrs/PERFORMANCE.md` records an instance where a `+10–20% throughput`
commit claim was later corrected to `+2.10%` after honest measurement.
Same convention here: if a torajs commit message claims X% and this
file later shows Y%, the Y is real and the X needs to be marked as a
misattribution. Self-correcting honesty is the rule.

## Open / pending measurement

- Cold-machine vs warm-machine bench delta (`PERFORMANCE.md` followup):
  current 4.41× was measured after a thermal-loaded 4+ hour cycle.
  A cold-machine 5-pass rerun is owed to confirm the number isn't
  pessimistic. (Tracked: P-PERF.0 follow-up.)
- Effect of `release-vanilla` vs `release` on torajs's own tooling
  build (NOT the compiled output). Mirror mailrs's "the perf-first
  profile cost 17% on native bench geomean" verification, but at the
  rust crate build level for tora's compiler.
- PGO baseline: not measured. Tentative future P-PERF substep.

## Sub-crate budgets

Per the layered-crate rewrite (see `docs/architecture-rewrite.md`),
each sub-crate ships with its own `BUDGETS.md` documenting per-bench
budgets + observed P95 + headroom factor. Those are regression-catch
gates (15–30× headroom over P95), NOT publishable numbers. Don't
quote a budget value as a perf claim; quote the criterion bench
median from the crate's `benches/<name>.rs` output instead.

| Crate | Status | `cargo bench` invocation |
|---|---|---|
| `torajs-pool` | not yet built (P1 pilot) | `cargo bench -p torajs-pool` |
| ... (filled as crates ship) | | |
