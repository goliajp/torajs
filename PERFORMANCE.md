# torajs perf — what's measured

torajs's positioning: AOT TypeScript runtime that dominates JS incumbents by
measured wall-clock. Every number below traces back to a measurement that
anyone can reproduce with the commands listed — not an estimate.

## Measured

### Workspace-level

| Path | Measurement | Run command |
|---|---|---|
| **geomean speedup vs bun-aot** | **4.41×** (5-pass median, sequential interleave, no concurrent test262/conformance run, mac M-series, host CPU detection enabled) | `cargo run -p bench-harness --release -- run --runs 5` |
| **geomean speedup vs bun-jsc** | **4.52×** | same |
| **geomean speedup vs node-v8** | **21.07×** | same |
| **per-case wins** | **26 / 26** vs bun-best | same |
| **conformance subset** | **666 / 0 / 1** (pass / fail / skip) | `cargo run --release --bin torajs-conformance` |
| **test262 in-scope pass rate** | **12.20%** (3455 / 28314) | |
| **AOT binary size (popcount case)** | ~36 KB stripped | `du -h target/release/<case>-aot` after `tr build` |

### Per-case bench medians (5-pass median, M-series Mac)

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
`ackermann` 1.8×, `fib40` 2.5×) are CPU-bound integer/float loops where
bun's JIT closes the gap. Wide leads (popcount 22.7×, fifo 7.6×, stack-pop
7.8×) show torajs's structural AOT advantage on tight typed loops.

## Reproduction protocol

Every published number above must be reproducible by running the commands
listed, on a quiet host (no concurrent test262 / conformance runs, no
thermal throttle), using the **5-pass median** protocol. Single-run mac
numbers carry a ±20–40% noise band and are not used for published claims.

## Sub-crate budgets

Each runtime sub-crate ships its own `BUDGETS.md` (see `crates/*/BUDGETS.md`)
documenting per-bench budgets, observed P95, and headroom factor. Those are
regression-catch gates, not publishable numbers — quote the criterion bench
median from the crate's `benches/<name>.rs` output instead of a budget value.
