# bench/

Cross-runtime perf benchmark module for torajs. Compares idiomatic, equivalent programs across **Bun, Node.js, Rust, Go, Python**, and (later) torajs itself.

> See `.claude/rfcs/20260426-bench-harness.md` for the design rationale.

## Quick start

```bash
cargo run -p bench-harness -- list           # list cases and detect runtimes
cargo run -p bench-harness -- run startup    # run one case
cargo run -p bench-harness -- run            # run all cases
```

Missing runtimes are auto-skipped with a notice. Results are appended to `bench/results/<date>-<host>-<git>.json`.

## Layout

```
bench/
├── cases/<case>/
│   ├── README.md           ← what the case stresses; per-language notes
│   ├── expected.txt        ← canonical stdout; mismatch fails the run
│   ├── main.ts             ← bun + node
│   ├── main.py
│   ├── main.rs
│   ├── main.go
│   └── (later) main.tora.ts
├── runners/<runtime>.toml  ← declarative: how to detect / compile / run
├── harness/                ← Rust binary that orchestrates everything
└── results/                ← committed JSON history (one file per run)
```

## Adding a case

1. `mkdir bench/cases/<name>` and write a `README.md` describing what it measures.
2. Drop language sources: `main.ts`, `main.rs`, `main.py`, `main.go` (skip what you don't have — the runner auto-skips).
3. Run any single source manually, copy its stdout to `expected.txt`. **All languages must produce byte-identical stdout.**
4. `cargo run -p bench-harness -- run <name>` — verifies stdout match across runtimes, then times them.

## Adding a runtime

Drop a TOML file at `bench/runners/<name>.toml`:

```toml
name = "deno"
detect = "deno --version"          # nonzero / not-on-PATH → auto-skip
src_filename = "main.ts"           # what file to look for in each case dir
compile = "deno compile {src} -o {out}"   # optional
run = "{out}"
```

Templates `{src}`, `{out}`, `{case}` are substituted before each invocation.

## Methodology notes

- Wall-clock timing comes from **hyperfine** (`--warmup 3 --runs 10`, median reported).
- Stdout is verified once per (case, runtime) before timing — this catches "we benchmarked the wrong program" silently.
- Compile time and run time are measured **separately** for compiled languages.
- Each result row carries the runtime version string — old result files stay interpretable later.
- Numbers are valid only for `(case, machine, runtime version, git SHA)` — never quote a number without all four.

## What's NOT in scope here

- CI-runner perf gates (too noisy on shared hardware).
- Auto-charts / dashboards.
- Codegolf / hand-tuned implementations per language. We benchmark **idiomatic** code.
- Async / HTTP / network workloads (re-add once torajs has async — P5+).
