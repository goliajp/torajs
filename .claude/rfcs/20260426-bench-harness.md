---
title: bench-harness — cross-runtime perf benchmark module
date: 2026-04-26
author: takagi
status: proposed
---

# Context

torajs hard requirement #1 (`docs/roadmap.md` — "极致 perf — beat Bun/Node/etc on important benchmarks; hold them") is currently unmeasurable. There is no apparatus to compare torajs against the runtimes it claims to outpace, and there is no baseline of where those runtimes sit relative to each other on workloads we care about. Until that scoreboard exists, the perf claim is decorative.

This RFC proposes a long-lived, cross-runtime benchmark module, `bench/`, with two delivery surfaces:

1. **In-repo module** — runnable with one command, auto-detects available runtimes.
2. **Skill** (later, dotclaude-side) — `/bench` wraps the module so an agent can invoke it without remembering the command.

The module is expected to live for the entire duration of the project. Old result snapshots are committed to git as a permanent perf record.

# Goals

1. Stand up a baseline today comparing **Bun, Node.js, Rust, Go, Python** on five workloads. Lock numbers before torajs has anything to measure.
2. Make the harness `cargo run -p bench-harness -- run` simple. No pre-flight setup beyond having the runtimes installed. Missing runtimes auto-skip with a notice — never fail the run.
3. Output is reproducible: same harness + same machine + same git SHA → same result file (modulo statistical noise within stated MAD).
4. Result history lives in `bench/results/` as JSON, committed to git, so future-us can do regression checks against past selves.
5. Each result row carries the runtime version string, so a result file is interpretable a year from now.

# Non-goals

- ❌ CI-runner perf gates. Shared CI runners are too noisy for credible perf numbers; perf gates only run on dedicated hardware (offline / opt-in). Out of scope.
- ❌ Auto-generated charts / dashboards. Plain JSON + terminal table is enough until we have years of data.
- ❌ Hand-tuned, codegolfed implementations per language. We benchmark **idiomatic** code, not maximum-effort. Any deviation is documented in the case README.
- ❌ Test262 conformance, TypeScript faithfulness, or anything that runs the *same TS source* through bun/node and through a future torajs build. The bun/node TS variant and the torajs variant may diverge; the case README declares each language's source explicitly.
- ❌ Async / HTTP / network workloads. Those are meaningless before P5 (async/await). Add later.
- ❌ JIT warmup engineering. We run hyperfine with `--warmup 3`; that's the contract. We don't try to coerce V8 / JSC into "tier-up" beyond what hyperfine's warmup happens to give us.

# Approach

## Repository layout

```
bench/
├── README.md                    ← how to run, how to add a case, how results are interpreted
├── cases/
│   ├── fib40/
│   │   ├── README.md            ← what this benchmarks, expected output, source notes per lang
│   │   ├── expected.txt         ← canonical stdout, all lang impls must match (validated each run)
│   │   ├── fib.ts               ← bun + node
│   │   ├── fib.rs
│   │   ├── fib.go
│   │   ├── fib.py
│   │   └── (later) fib.ts       ← torajs source, may diverge from bun/node version (separate file)
│   ├── mandelbrot/
│   ├── json-parse-1mb/
│   ├── string-concat-1m/
│   └── startup/
├── runners/
│   ├── bun.toml                 ← declarative: how to compile (if any) + how to run, per language
│   ├── node.toml
│   ├── rust.toml
│   ├── go.toml
│   ├── python.toml
│   └── (later) torajs.toml
├── harness/                     ← Rust binary crate, member of the workspace
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── runner.rs            ← parse runners/*.toml, detect availability, build commands
│       ├── case.rs              ← discover cases/*, validate expected.txt
│       ├── bench.rs             ← shell out to hyperfine, parse JSON output
│       └── report.rs            ← render terminal table + write results/*.json
└── results/
    └── 2026-04-26-<host>-<git>.json
```

The harness is a Rust crate. It runs on the developer's machine (not in the engine itself). `runners/*.toml` is data, not code, so adding a new language is editing TOML + dropping a source file in each case dir.

## Workspace

Add a top-level `Cargo.toml` workspace at repo root:

```toml
[workspace]
resolver = "3"
members = [
    "labs/0001-walking-skeleton",
    "bench/harness",
]
```

This shares `target/`, makes `cargo run -p tr` and `cargo run -p bench-harness` symmetric, and is the right structure for when `crates/` populates later.

## Runner descriptor format

`runners/rust.toml`:
```toml
name = "rust"
detect = "rustc --version"        # absent / nonzero exit → skip with notice
ext = "rs"
compile = "rustc -O {src} -o {out}"
run = "{out}"
binary_size_path = "{out}"        # for size metric; omit if interpreted
```

`runners/bun.toml`:
```toml
name = "bun"
detect = "bun --version"
ext = "ts"
run = "bun run {src}"             # no compile step
```

The harness fills `{src}`, `{out}`, etc., per case. Compile time and run time are measured separately via two hyperfine invocations.

## Metrics per case × runtime

| metric | how |
|---|---|
| `compile_ms` | hyperfine `--warmup 1 --runs 5` over the compile command (compiled langs only) |
| `run_ms` | hyperfine `--warmup 3 --runs 10` over the run command, median |
| `run_mad_ms` | hyperfine median absolute deviation |
| `peak_rss_kb` | `/usr/bin/time -l` on macOS, `-v` on Linux, parsed |
| `binary_bytes` | `stat` of the compile output (compiled langs only) |
| `runtime_version` | output of `detect` command, captured once per run |
| `stdout_match` | bool, `actual_stdout == expected.txt` (any false fails the run) |

## Case acceptance rules

A case is valid iff:

1. `expected.txt` exists and is non-empty.
2. Every language source under `cases/<name>/` produces stdout exactly equal to `expected.txt` when run through its declared runner (validated by harness on every run).
3. `README.md` exists describing what the case stresses and any per-language deviations.

A case can ship without all five language impls — a missing impl just means that runtime is not measured for that case. This lets us add languages incrementally (and add torajs later without holding everything else).

## First five cases

1. **fib40** — `fib(40)` recursive. Stresses function call overhead.
2. **mandelbrot** — 1024×1024 escape time, prints checksum. Tight numeric loops.
3. **json-parse-1mb** — parse a fixed 1 MB JSON file (committed to case dir), print one field. Stresses stdlib parse path.
4. **string-concat-1m** — build a 1M-char string by concatenation, print length. Stresses string memory model.
5. **startup** — empty `console.log("x")` / `print("x")` / `println!("x")`. Pure startup overhead.

## CLI surface

```bash
cargo run -p bench-harness -- list                     # list cases
cargo run -p bench-harness -- run                      # run all cases × all available runtimes
cargo run -p bench-harness -- run fib40                # one case
cargo run -p bench-harness -- run --runtime rust,bun   # filter runtimes
cargo run -p bench-harness -- run --no-save            # don't write results/
```

Default: writes to `bench/results/<date>-<host>-<git>.json` and prints a markdown-style table to stdout.

## Skill (deferred)

A `/bench` skill in dotclaude that wraps the above commands and pretty-prints results into the conversation. Not part of this RFC's implementation scope; recorded as a follow-up. The in-repo module is designed so the skill is a 30-line wrapper.

# Affected files

**New:**

- `Cargo.toml` (root workspace)
- `bench/README.md`
- `bench/harness/Cargo.toml`
- `bench/harness/src/{main,runner,case,bench,report}.rs`
- `bench/runners/{bun,node,rust,go,python}.toml`
- `bench/cases/<each>/README.md`
- `bench/cases/<each>/expected.txt`
- `bench/cases/<each>/<src>.{ts,rs,go,py}`
- `bench/results/.gitkeep`

**Edited:**

- `docs/roadmap.md` — add BENCH track section + decision-1 note (TS6 path explicitly rejected) + add P11.4 self-hosted linter (decision-3, separate from this RFC's bench scope but bundled in the same commit since it's a single-line addition).
- `CLAUDE.md` — workspace structure note (top-level `Cargo.toml` now exists).
- `labs/0001-walking-skeleton/Cargo.toml` — no change required; will be picked up as workspace member.

# Test cases (for the harness itself)

Not unit tests — the harness's correctness is observable by running it. Acceptance criteria:

1. `cargo run -p bench-harness -- list` prints the five cases.
2. `cargo run -p bench-harness -- run startup` succeeds on this dev machine, produces a JSON file, and the JSON contains rows for at least bun/node/rust/go/python with `stdout_match = true`.
3. With `node` uninstalled (simulated by renaming PATH), the run skips node with a notice and exits 0.
4. Manually corrupting one language's source so its stdout differs from `expected.txt` causes `stdout_match = false` and the run exits nonzero.
5. Two consecutive runs of `startup` on the same machine produce results within ±15% wall-clock of each other (sanity check on hyperfine warmup adequacy).

# Risks

| risk | mitigation |
|---|---|
| **Numbers get cited out of context** ("torajs is 2× faster than Node!"). | README.md preface: results are *only* meaningful for the case + machine + version recorded. Never quote a number without those three. |
| **Idiomatic-code definition is contentious.** | Each case's README.md declares per-language deviations explicitly. PR review is the gate. |
| **macOS ↔ Linux time(1) flag divergence.** | Detect OS in harness; parse `/usr/bin/time -l` (mac) vs `-v` (linux) in distinct paths. |
| **hyperfine becomes unmaintained.** | Pinned at 1.20+; if it breaks, swap to in-process `Instant::now()` measurement (15-30 LOC of code). |
| **Adding torajs later means rerunning everything for fair comparison.** | That's fine. Result files are dated; old runs stay archived. |
| **Bun/Node ts source identical, torajs source diverges.** | Each case dir holds two `.ts` files when needed: `<name>.ts` (bun/node) and `<name>.tora.ts` (torajs). Documented per-case. |

# Open questions (resolve in implementation)

1. **Compile time as a metric for ts on bun/node**: bun does in-process transpilation, node usually pairs with `tsx` / `swc-node`. Decision: for first cut, treat bun/node as "no compile step" (compile time = 0) — i.e., pay any transpile cost inside `run_ms`. Revisit if it distorts comparisons.
2. **Where does the JSON test fixture for `json-parse-1mb` come from?** Generate deterministically in-repo from a seed (committed Python script) so the file is reproducible without committing a 1 MB blob.
3. **Should `results/` be `.gitignore`d?** No — committing them is the whole point (perf history). But .gitignore noisy auto-generated experimental runs (`*-scratch.json`).

# Out of scope (for this RFC, not for the project)

- Adding more case categories (concurrent, async, http, db) — separate RFC at P5+.
- The `/bench` skill in dotclaude — separate work, separate repo.
- Perf-gate tests inside individual crates (per `.claude/rules/rust/patterns.md`) — those are crate-internal microbenchmarks, different layer from this cross-runtime harness. Both will coexist.
