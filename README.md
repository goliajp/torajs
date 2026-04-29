# torajs

A TypeScript runtime that runs a subset of TS programs with **TS semantics**, AOT-compiled to small native binaries via LLVM or JIT-executed via Cranelift. **No GC**, no refcount — the compiler infers ownership at compile time.

> Closed-source research project. Public site: https://torajs.com

## What it is

```
TS subset surface              TS semantics, no GC                Two real codegens
──────────────────             ───────────────────────────        ─────────────────
function fib(n: number)        let n = s; console.log(s);         tr build  → LLVM 22
  : number { ... }             // both work, one drop fires        tr run    → Cranelift JIT
let s: string = "..."          // (compile-time ownership)         same SSA IR feeds both
```

bun is the oracle. When behavior is unclear, write the equivalent in TS, run it in `bun`, and match.

`number` is `i64` by default; `f64` is opt-in. Strings, objects, and arrays follow TS reference semantics — multiple bindings can alias the same heap, the compiler picks the owner statically and emits one drop. No `null`, no `==`, no `var`, no decorators, no `eval`. Differentiator from bun is the runtime: 30-something-KB native binary, ~1.2 ms startup, no GC pauses. See [`docs/ts-subset.md`](docs/ts-subset.md) for the supported subset boundary.

## Bench scoreboard

Cross-runtime perf, M4 Pro, hyperfine n=3-10. Run times in ms (lower better). [Full data](bench/results/).

| case | torajs (AOT) | torajs-jit | rust | go | bun-jsc | bun-aot | node-v8 |
|---|---:|---:|---:|---:|---:|---:|---:|
| ackermann |  **8.67** ← |  18.31 |  8.85 |  10.55 | 15.84 | 16.23 |  91.23 |
| collatz   | 104.77 | 207.68 | **103.82** | 136.68 | 320.88 | 321.34 | 1397.41 |
| fib40 | **147.18** ← | 519.17 | 178.82 | 224.77 | 378.72 | 372.81 |  644.28 |
| gcd1m |  **39.05** ← |  50.54 | 39.93 |  40.60 |  48.41 |  48.18 |  126.69 |
| mandelbrot |  34.30 |  86.44 | **33.80** | 36.18 | 49.98 | 50.57 |  125.97 |
| popcount |   **2.60** ← | 100.75 |  2.79 |  55.04 |  55.57 |  57.16 |  129.53 |
| prime_count | 48.01 |  55.40 | 48.13 | **39.94** |  54.01 |  56.53 |  162.36 |
| startup |   **1.21** ← |   7.70 |  1.56 |   1.84 |   8.01 |   8.19 |   81.44 |

Measured 2026-04-30 post-TS-subset-pivot. No regression vs pre-pivot (within ±6% noise).

torajs (AOT) **vs rust**: 5 wins, 3 ties, 0 losses (largest "loss" = 1.5% on mandelbrot, within stddev).
torajs (AOT) **vs go**: 7 wins, 1 loss (prime_count's trial division — go's GC backend is fast on tight int loops).
torajs (AOT) **vs bun/node**: 8/8 wins on every case. `popcount 2.60 ms vs bun-jsc's 55.57 ms = 21.4× faster`. `startup 1.21 ms vs node-v8's 81.44 ms = 67× faster`. fib40 vs bun-jsc: **2.57×**. collatz vs bun-jsc: **3.06×**.

Compile time + binary size:

| | compile_ms | binary |
|---|---:|---:|
| **torajs (AOT)** | **~43** | **33.9 KB** ← |
| go | ~37 | 2.37 MB |
| bun-aot | ~58 | 63 MB |
| rust | ~73 | 466 KB |

torajs binary is **14× smaller** than rust, **70× smaller** than go, **1860× smaller** than bun-aot — small enough to fit in an L2 cache.

## Architecture

```
                       ┌─────────────────────┐
                       │  source.tora.ts     │
                       └──────────┬──────────┘
                                  │
                           lex / parse / check
                                  │
                       ┌─────────────────────┐
                       │  SSA IR             │ ← rich type info, alias-aware
                       │  (ssa.rs, ssa_lower)│   ownership inference, intrinsics
                       └──────────┬──────────┘
                                  │
                  ┌───────────────┴───────────────┐
                  │                               │
            tr build                          tr run
                  │                               │
       ┌──────────────────┐             ┌──────────────────┐
       │  Inkwell (LLVM 22)│             │  Cranelift JIT  │
       │  AOT + cc link    │             │  in-process     │
       └──────────────────┘             └──────────────────┘
                  │                               │
            33 KB binary                  in-memory page
            run-leading codegen           ~5 ms compile
```

One frontend. One IR. Two backends sharing the same lowering. `tr build` is the production path (perf-leading). `tr run` is the dev loop (fast compile, immediate execution — same shape as `go run`).

## Quick start

Requires Rust nightly + LLVM 22 (homebrew):

```bash
brew install llvm                                # LLVM 22
git clone git@github.com:goliajp/torajs.git
cd torajs

LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm \
  cargo build --release -p tr -p bench-harness

# Run a program (Cranelift JIT)
echo 'console.log("hello");' > hi.tora.ts
./target/release/tr run hi.tora.ts

# AOT-compile to native binary (LLVM)
./target/release/tr build hi.tora.ts -o hi
./hi

# Run the cross-runtime bench
./target/release/bench-harness run
```

## Status — TS subset core in progress (M1)

| milestone | what | status |
|---|---|---|
| **P0/P1** | walking skeleton + core language (arithmetic, control flow, fns, strings, arrays) | ✓ |
| **P2.1+** | alias-aware ownership inference (no GC, TS-shape shared reads) | ✓ |
| **P2.2** | string runtime + drop emission (concat doesn't consume) | ✓ |
| **P2.4** | object literals + structural types | ✓ |
| **P3** | LLVM AOT + Cranelift JIT, two backends sharing one SSA IR | ✓ |
| **stdlib slice 1** | `console.log`, `Math.*`, `String.length`, `print_f64` | ✓ |
| **M1** | TS subset core completeness (comments, Array runtime, block-scope drops, mutable struct fields, bool ops, for/break/continue) | in progress |
| **M2** | closures with implicit captures | next |
| **M3** | generics in user code | — |
| **M4** | error model: try/catch/throw | — |
| **M5** | module system + graduation to `crates/` (v0.1) | — |
| **M6** | bun-shape stdlib expansion (String/Array methods, Date, JSON, fs, Bun.*) | — |
| **M7** | async/await + single-threaded executor | — |
| **M8** | playground (wasm) + LSP + tooling | — |
| **M9** | source maps + debugger + embedding API + multi-thread → v1.0 | — |

[Full roadmap](docs/roadmap.md). Phase P3 went through a mid-project pivot from wasm-via-C → LLVM-direct + Cranelift. The P2 ownership story was framing-corrected on 2026-04-30 (TS subset, not Rust dialect; see [`docs/ts-subset.md`](docs/ts-subset.md)).

## Bench cases

Each case has a `main.tora.ts` + 5 sibling implementations (`main.ts`, `main.rs`, `main.go`, `main.py`) for cross-language comparison.

| case | what it stresses |
|---|---|
| **fib40** | recursion, integer arithmetic |
| **popcount** | LLVM loop-idiom recognition (BK pattern → ARM `cnt.16b` NEON) |
| **gcd1m** | tight integer loop with mod |
| **mandelbrot** | f64 nested loops, FMA tolerance |
| **startup** | program launch cost (JIT warmup vs cold-start) |
| **ackermann** | nested recursion (recursive call as another's argument) |
| **collatz** | bit ops + hailstone trajectory + outer/inner loop |
| **prime_count** | trial division, bool return, early-return-from-while |

Add a case: drop a directory under `bench/cases/<name>/` with `main.<lang>` files, an `expected.txt`, and an optional `bench.toml`.

## Project layout

```
torajs/
├── labs/0001-walking-skeleton/  ← the compiler (~4500 LOC of Rust)
│   ├── src/lexer.rs
│   ├── src/parser.rs
│   ├── src/check.rs              ← typechecker + alias-aware ownership inference
│   ├── src/ssa.rs                ← SSA IR types + pretty printer
│   ├── src/ssa_lower.rs          ← AST → SSA
│   ├── src/ssa_inkwell.rs        ← SSA → LLVM 22 (Inkwell)
│   └── src/ssa_cranelift.rs      ← SSA → Cranelift CLIF (JIT)
├── labs/0002-inkwell-spike/     ← throwaway: LLVM gate validation
├── bench/                        ← cross-runtime perf harness
├── docs/roadmap.md               ← canonical implementation plan
└── web/                          ← torajs.com website (Vite + React)
```

`labs/` is intentionally throwaway-friendly. Code graduates to `crates/` when it stabilizes.

## Conventions

- **Branches**: `develop` is the active branch (no `main` until first release)
- **Commits**: lowercase types (`feat: ...`, `fix: ...`), no AI co-author tags
- **Languages**: Chinese for design discussion; English for code, comments, commits, docs

## License

Closed-source research project. Code is not open for redistribution.
