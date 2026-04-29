# torajs

A statically-typed TS-shaped language with Rust-shaped semantics — AOT-compiled to native binaries via LLVM, JIT-executed via Cranelift, no tracing GC.

> Closed-source research project. Public site: https://torajs.com

## What it is

```
TS-shaped surface              Rust-shaped semantics              Two real codegens
──────────────────             ─────────────────────              ─────────────────
function fib(n: number)        affine types, no tracing GC        tr build  → LLVM 22
  : number { ... }             explicit Rc<T> for shared          tr run    → Cranelift JIT
let s: string = "..."          deterministic drop                 same SSA IR feeds both
```

`number` is `i64` by default; `f64` is opt-in. `string` is heap-owned with affine ownership — `let b = a` moves `a`, subsequent reads error at compile time. No `null`, no `==`, no `var`, no decorators, no `eval`, no Test262 conformance.

## Bench scoreboard

Cross-runtime perf, M4 Pro, hyperfine n=3-10. Run times in ms (lower better). [Full data](bench/results/).

| case | torajs (AOT) | torajs-jit | rust | go | bun-jsc | bun-aot | node-v8 | python |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| ackermann |  **8.58** ← |  17.60 |  8.75 |  9.62 | 15.06 | 15.58 |  101.35 |   96.80 |
| collatz   | 99.11 | 202.05 | **98.43** | 128.99 | 304.42 | 306.36 | 1354.36 | 4742.57 |
| fib40 | **150.32** ← | 514.15 | 178.56 | 227.26 | 382.48 | 383.44 |  641.08 | 6465.04 |
| gcd1m |  **38.05** ← |  50.06 | 39.37 |  38.78 |  46.06 |  47.69 |  128.78 |  307.36 |
| mandelbrot |  34.21 |  88.25 | **33.61** | 35.40 | 49.20 | 48.45 |  121.45 | 1081.11 |
| popcount |   **2.65** ← | 105.13 |  2.89 |  51.97 |  56.41 |  55.27 |  129.74 | 2808.20 |
| prime_count | 47.74 |  55.45 | 47.67 | **40.32** |  54.51 |  52.37 |  159.45 | 1784.70 |
| rc-clone-1m |   **4.06** ← |  35.75 |  4.43 |    — |    — |    — |      — |       — |
| startup |   **1.15** ← |   8.02 |  1.34 |   1.82 |   7.86 |   7.64 |   81.50 |   16.59 |

torajs (AOT) **vs rust**: 6 wins, 3 ties, 0 losses (largest "loss" = 1.8% on mandelbrot, within stddev).
torajs (AOT) **vs go**: 7 wins, 1 loss (prime_count's trial division — go's gc backend is fast on tight int loops).
torajs (AOT) **vs bun/node**: 8/8 wins on every case where they have an entry. `popcount 2.65 ms vs node-v8's 129.74 ms = 49× faster`. `startup 1.15 ms vs 81.50 ms = 71×`.

`rc-clone-1m` is Rc-specific: bun/node/go/python don't have explicit refcount primitives, only torajs and rust ship a runner. torajs's foreign-call dispatch to `__torajs_rc_clone` keeps the inc/dec from being cancelled by LLVM, while Rust inlines `Rc::clone` and re-introduces the cost via `black_box` — both end up doing the same physical work. Result: torajs **0.92× of rust**, well inside the P2.3.d gate (within 1.2×).

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
                       │  SSA IR             │ ← rich type info, affine ownership
                       │  (ssa.rs, ssa_lower)│   tracking, intrinsic calls
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

## Status — P3 closed, P2 in progress

| phase | what | status |
|---|---|---|
| **P0** | walking skeleton: `tr run hello.ts` prints `hello` | ✓ |
| **P1** | core language (arithmetic, vars, control flow, fns, strings, arrays) | ✓ |
| **P2.1** | affine types — use-after-move is a type error | ✓ |
| **P2.2** | strings as values: alloc, print, drop, concat | ✓ |
| **P2.3** | `Rc<T>` first-class | — |
| **P2.4** | object literals + structural types | — |
| **P3** | LLVM AOT + Cranelift JIT, two backends sharing one SSA IR | ✓ |
| **P4** | closures with ownership analysis | — |
| **P5** | async/await | — |
| **P6** | Send/Sync, multi-core executor | — |

[Full roadmap](docs/roadmap.md). Phase P3 went through a mid-project pivot from wasm-via-C → LLVM-direct + Cranelift — see commits `0f84fb8` (decision) and `61ae24a` / `5aa9c96` (P3.7 + P3.6 closeout).

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
| **rc-clone-1m** | `Rc<T>::clone` + drop hot path; refcount cost vs Rust's `Rc::clone` (torajs-only + rust; other rows skip) |

Add a case: drop a directory under `bench/cases/<name>/` with `main.<lang>` files, an `expected.txt`, and an optional `bench.toml`.

## Project layout

```
torajs/
├── labs/0001-walking-skeleton/  ← the compiler (~4500 LOC of Rust)
│   ├── src/lexer.rs
│   ├── src/parser.rs
│   ├── src/check.rs              ← typechecker + affine pass
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
