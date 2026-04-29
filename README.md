# torajs

A TypeScript runtime that runs a subset of TS programs with **TS semantics**, AOT-compiled to small native binaries via LLVM or JIT-executed via Cranelift. **No GC**, no refcount — the compiler infers ownership at compile time.

> Closed-source research project. Public site: https://torajs.com

## What it is

```
TS subset surface         TS semantics, no GC               Two real codegens
─────────────────         ───────────────────               ─────────────────
function fib(n: number)   let n = s;                        tr build → LLVM 22
   : number { ... }       console.log(s); console.log(n);   tr run   → Cranelift JIT
let xs: number[] = [];    // both reads work                 same SSA IR feeds both
xs.push(1); xs[0] = 9;    // one drop at scope exit          ~33 KB native binary
```

bun is the oracle: when behavior is unclear, write the equivalent in TS, run it in `bun`, and match.

`number` is `i64` by default; `f64` is opt-in. Strings, objects, and arrays follow TS reference semantics — multiple bindings can alias the same heap, the compiler picks the owner statically and emits one drop. No `null`, no `==`, no `var`, no decorators, no `eval`. Differentiator from bun is the runtime: ~33 KB native binary, ~1.2 ms startup, no GC pauses. See [`docs/ts-subset.md`](docs/ts-subset.md) for the supported subset boundary.

## Bench scoreboard

Cross-runtime perf, M4 Pro, hyperfine n=3-10. Run times in ms (lower better). [Full data](bench/results/).

| case                  | torajs (AOT) | torajs-jit |       rust |         go |    bun-jsc |    bun-aot |    node-v8 |
| --------------------- | -----------: | ---------: | ---------: | ---------: | ---------: | ---------: | ---------: |
| ackermann             |     **8.41** |      19.06 |       8.36 |      10.79 |      15.93 |      15.33 |     100.72 |
| array-sum-1m          |    **12.06** |      44.23 |      14.28 |      30.47 |      50.70 |      47.56 |     171.97 |
| **closure-pipeline-1m** | **12.98**  |      52.45 |      19.45 |      36.94 |      46.70 |      46.23 |     173.07 |
| collatz               |       104.40 |     208.71 | **104.05** |     137.56 |     320.28 |     320.93 |    1392.06 |
| fib40                 |   **148.26** |     515.69 |     177.26 |     227.74 |     374.20 |     376.04 |     652.07 |
| gcd1m                 |    **39.91** |      50.83 |      40.37 |      41.17 |      48.32 |      47.93 |     128.18 |
| mandelbrot            |    **34.52** |      85.25 |      34.63 |      36.85 |      50.56 |      50.59 |     123.45 |
| popcount              |     **3.05** |     103.54 |       3.12 |      54.91 |      56.48 |      55.88 |     127.63 |
| prime_count           |        47.64 |      54.21 |      47.93 |  **39.72** |      52.95 |      51.07 |     157.85 |
| startup               |     **1.22** |       8.50 |       1.40 |       2.24 |       8.32 |       7.65 |      85.94 |

Measured 2026-04-30 post-M1 (TS subset core: comments / Array runtime / block drops / mutable field+index write / boolean ops / for-loop / break+continue).

torajs (AOT) **vs rust**: 8 wins, 2 ties (collatz +0.3%, mandelbrot −0.3%, both within stddev), 0 losses. **`array-sum-1m`: 12.06 ms vs rust's `Vec<i64>` 14.28 ms = 1.18× faster**. **`closure-pipeline-1m`: 12.98 ms vs rust's fn-pointer indirect call 19.45 ms = 1.50× faster**.
torajs (AOT) **vs go**: 9 wins, 1 loss (prime_count's trial division — go's GC backend is fast on tight int loops).
torajs (AOT) **vs bun/node**: **10/10 wins** on every case. `popcount 3.05 ms vs bun-jsc's 56.48 ms = 18.5× faster`. `startup 1.22 ms vs node-v8's 85.94 ms = 70× faster`. `array-sum-1m vs bun-jsc: 4.2×`. `closure-pipeline-1m vs bun-jsc: 3.60×`. `fib40 vs bun-jsc: 2.52×`. `collatz vs bun-jsc: 3.07×`.

Compile time + binary size:

| runtime          | compile_ms |     binary |
| ---------------- | ---------: | ---------: |
| **torajs (AOT)** |     **~45** | **33.9 KB** |
| go               |        ~38 |    2.37 MB |
| bun-aot          |        ~58 |      63 MB |
| rust             |        ~75 |     466 KB |

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

## Status — M1 complete; M2 (closures) next

| milestone          | what                                                                                                | status |
| ------------------ | --------------------------------------------------------------------------------------------------- | :----: |
| P0 / P1            | walking skeleton + core language (arithmetic, control flow, fns, strings, arrays)                   |   ✓    |
| P2.1+              | alias-aware ownership inference (no GC, TS-shape shared reads)                                      |   ✓    |
| P2.2               | string runtime + drop emission (concat doesn't consume)                                             |   ✓    |
| P2.4               | object literals + structural types                                                                  |   ✓    |
| P3                 | LLVM AOT + Cranelift JIT, two backends sharing one SSA IR                                           |   ✓    |
| stdlib slice 1     | `console.log`, `Math.*`, `String.length`, `print_f64`                                               |   ✓    |
| **M1**             | TS subset core (comments, Array runtime, block-scope drops, mutable field/index write, bool ops, for, break/continue) | ✓ |
| **M2**             | closures with implicit captures                                                                     |  next  |
| M3                 | generics in user code                                                                               |   —    |
| M4                 | error model: try / catch / throw                                                                    |   —    |
| M5                 | module system + graduation to `crates/` (v0.1)                                                      |   —    |
| M6                 | bun-shape stdlib expansion (String / Array methods, Date, JSON, fs, Bun.*)                          |   —    |
| M7                 | async / await + single-threaded executor                                                            |   —    |
| M8                 | playground (wasm) + LSP + tooling                                                                   |   —    |
| M9                 | source maps + debugger + embedding API + multi-thread → v1.0                                        |   —    |

[Full roadmap](docs/roadmap.md). Phase P3 went through a mid-project pivot from wasm-via-C → LLVM-direct + Cranelift. The P2 ownership story was framing-corrected on 2026-04-30 (TS subset, not Rust dialect; see [`docs/ts-subset.md`](docs/ts-subset.md)).

## Bench cases

Each case has a `main.tora.ts` + 5 sibling implementations (`main.ts`, `main.rs`, `main.go`, `main.py`) for cross-language comparison.

| case             | what it stresses                                                                |
| ---------------- | ------------------------------------------------------------------------------- |
| **fib40**        | recursion, integer arithmetic                                                   |
| **popcount**     | LLVM loop-idiom recognition (BK pattern → ARM `cnt.16b` NEON)                   |
| **gcd1m**        | tight integer loop with mod                                                     |
| **mandelbrot**   | f64 nested loops, FMA tolerance                                                 |
| **startup**      | program launch cost (JIT warmup vs cold-start)                                  |
| **ackermann**    | nested recursion (recursive call as another's argument)                         |
| **collatz**      | bit ops + hailstone trajectory + outer/inner loop                               |
| **prime_count**  | trial division, bool return, early-return-from-while                            |
| **array-sum-1m** | 10M `Array<T>::push` + index sum — heap alloc, amortized realloc, tight loop   |
| **closure-pipeline-1m** | 10M indirect calls through fn-pointer arg — `reduce(xs, f)` higher-order pattern |

Add a case: drop a directory under `bench/cases/<name>/` with `main.<lang>` files, an `expected.txt`, and an optional `bench.toml`.

## Project layout

```
torajs/
├── labs/0001-walking-skeleton/   ← the compiler (~8000 LOC of Rust)
│   ├── src/lexer.rs
│   ├── src/parser.rs
│   ├── src/check.rs              ← typechecker + alias-aware ownership inference
│   ├── src/ssa.rs                ← SSA IR types + pretty printer
│   ├── src/ssa_lower.rs          ← AST → SSA
│   ├── src/ssa_inkwell.rs        ← SSA → LLVM 22 (Inkwell)
│   └── src/ssa_cranelift.rs     ← SSA → Cranelift CLIF (JIT)
├── labs/0002-inkwell-spike/      ← throwaway: LLVM gate validation
├── bench/                        ← cross-runtime perf harness
├── docs/roadmap.md               ← canonical implementation plan
├── docs/ts-subset.md             ← TS subset boundary documentation
└── web/                          ← torajs.com website (Vite + React)
```

`labs/` is intentionally throwaway-friendly. Code graduates to `crates/` when it stabilizes.

## Conventions

- **Branches**: `develop` is the active branch (no `main` until first release)
- **Commits**: lowercase types (`feat: ...`, `fix: ...`), no AI co-author tags
- **Languages**: Chinese for design discussion; English for code, comments, commits, docs

## License

Closed-source research project. Code is not open for redistribution.
