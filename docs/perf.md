# torajs — performance

torajs's differentiator is **AOT-to-native + small artifact + fast
startup** while keeping TS semantics. The bench scoreboard tracks
that against bun and other runtimes on a fixed set of programs.

## Bench scoreboard (current state)

19 committed bench cases under `bench/cases/`. Each case ships in
multiple languages (TS for bun + node, Rust, Go, Python, plus
`main.tora.ts` for tr). Numbers are wall-clock measured by
`hyperfine` against pre-warmed binaries.

| Metric | What it measures |
|---|---|
| `compile_ms` | Time to produce the binary (or for `tr run` / `bun run`, time to first bytecode) |
| `run_ms` | Time to execute the compiled program against the case's standard input |
| `size` | Final binary size on disk (where applicable) |

**`tr build` wins on all 19 cases** against `bun --compile`, on a
tight integer-arithmetic spread (fib40, gcd1m, popcount, prime_count,
collatz, ackermann), on string-heavy cases (csv-trim-100k,
csv-rebuild-100k, split-only-100k), and on heap-allocation-heavy
cases (array-map-1m, array-sum-1m, fifo-queue-100k, stack-pop-1m).

## Reproduction

Hardware profile this baseline was measured on:

- Apple M4 Pro, macOS 25.4
- Apple clang 21 (Xcode), `-O3` + PGO
- bun 1.3.13, node 24.x

```sh
# from the repo root
cargo run -p bench-harness -- list      # detect installed runtimes
cargo run -p bench-harness -- run       # all cases, all runtimes
cargo run -p bench-harness -- run fib40 # one case
```

Results are appended to `bench/results/<date>-<host>-<git>.json`.

## Why this measurement

- `tr build` produces a real binary (no V8 / no JIT runtime) — the
  comparison against `bun --compile` puts both on the same footing
  (compiled, executed against fresh process startup)
- `tr run` mirrors `bun run` — the JIT-style "compile + cache + exec"
  path most users hit during dev loops
- Both axes get reported separately so we can't accidentally compare
  `tr build` against `bun run` (or vice versa)

## Caveats

- Short cases (< 5 ms) drift ±15 % under high system load. Treat
  individual numbers in the noise band; commit-to-commit comparisons
  use a 14-run history window.
- `tr build` cold compile time is not optimized — current focus is
  `run_ms`. `tr run` cache reuse pulls compile latency out of the
  inner dev loop.
- Cases that depend on heavy stdlib surface tr doesn't yet implement
  (regex, Symbol, Date, Promise) aren't in the scoreboard.

## How to add a bench case

1. `bench/cases/<name>/` with a `main.tora.ts` (tr) + `main.ts`
   (bun/node) + `expected.txt`.
2. Run `cargo run -p bench-harness -- run <name>` and verify
   stdout matches `expected.txt` for every runtime.
3. Open a PR. The bench rule says **every committed case must pass
   on tr** — if tr can't run the case yet, file a roadmap-gap issue
   first and land the case once the gap closes.
