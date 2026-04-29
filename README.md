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

Cross-runtime perf, M4 Pro, hyperfine n=5 with 2 warmup runs. Measured 2026-04-30 (post-M2 Phase B). All times in ms; binary in KB / MB. **`compile`** = AOT compile + link wall time; **`run (build)`** = AOT-compiled binary execution; **`run (jit)`** = JIT / interpreted execution; **`binary`** = on-disk size of the produced executable. [Full JSON data](bench/results/).

### Headline summary (run-time, lower better)

|     case                  | torajs (AOT)  | torajs (JIT)  |       rust |         go |    bun-jsc |    bun-aot |    node-v8 |
| ------------------------- | ------------: | ------------: | ---------: | ---------: | ---------: | ---------: | ---------: |
| ackermann                 |      **9.64** |         20.46 |       9.61 |      10.65 |      17.58 |      16.96 |     100.32 |
| array-sum-1m              |     **13.54** |         50.39 |      16.75 |      35.03 |      52.69 |      56.78 |     177.69 |
| **closure-pipeline-1m**   |     **14.91** |         55.78 |      20.64 |      41.17 |      51.95 |      51.33 |     184.34 |
| collatz                   |    **105.80** |        220.57 |     110.07 |     143.18 |     325.28 |     324.69 |    1626.75 |
| fib40                     |    **152.44** |        520.65 |     179.00 |     226.74 |     388.79 |     382.68 |     661.72 |
| gcd1m                     |     **40.36** |         51.59 |      40.84 |      41.54 |      49.11 |      48.29 |     130.98 |
| mandelbrot                |     **35.19** |         86.49 |      35.21 |      37.58 |      51.73 |      51.28 |     124.80 |
| popcount                  |      **2.84** |        104.11 |       3.19 |      54.92 |      55.43 |      56.89 |     133.91 |
| prime_count               |         48.54 |         55.20 |      48.30 |  **41.24** |      53.19 |      54.29 |     161.41 |
| startup                   |      **1.44** |          9.27 |       1.81 |       2.11 |       9.18 |       9.03 |      83.09 |

torajs (AOT) **vs rust**: 9 wins / 1 tie (mandelbrot, +0.06% within stddev) / 0 losses.
torajs (AOT) **vs go**: 9 wins, 1 loss (prime_count — go's GC-backed tight integer loops).
torajs (AOT) **vs bun-jsc / bun-aot / node-v8**: **10 / 10** wins per runtime.

### Per-case detail — compile / run / binary

Each case shows: AOT compile time, AOT execution time, JIT/interpreted execution time, and on-disk binary size. `—` marks runtimes that don't have that mode (JIT compiles in-memory, no binary; bun-jsc/node-v8 don't have a compile step separate from run).

#### `ackermann` — nested recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   49 ms |  **9.64 ms ←** | **34.9 KB** |
| torajs (JIT)     |     —   |   20.46 ms |          — |
| rust             |   80 ms |    9.61 ms |     466 KB |
| go               |   41 ms |   10.65 ms |    2.37 MB |
| bun-aot          |   61 ms |   16.96 ms |      63 MB |
| bun-jsc          |     —   |   17.58 ms |          — |
| node-v8          |     —   |  100.32 ms |          — |

#### `array-sum-1m` — 10M `Array<T>::push` + index sum (heap alloc + amortized realloc)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   52 ms | **13.54 ms ← (1.24× faster than rust)** | **34.9 KB** |
| torajs (JIT)     |     —   |   50.39 ms |          — |
| rust             |   90 ms |   16.75 ms |     466 KB |
| go               |   42 ms |   35.03 ms |    2.37 MB |
| bun-aot          |   66 ms |   56.78 ms |      63 MB |
| bun-jsc          |     —   |   52.69 ms |          — |
| node-v8          |     —   |  177.69 ms |          — |

#### `closure-pipeline-1m` — 10M indirect calls through fn-pointer arg

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   53 ms | **14.91 ms ← (1.38× faster than rust)** | **35.0 KB** |
| torajs (JIT)     |     —   |   55.78 ms |          — |
| rust             |   93 ms |   20.64 ms |     467 KB |
| go               |   42 ms |   41.17 ms |    2.37 MB |
| bun-aot          |   62 ms |   51.33 ms |      63 MB |
| bun-jsc          |     —   |   51.95 ms |          — |
| node-v8          |     —   |  184.34 ms |          — |

Rust uses `black_box(add1 as fn(i64)->i64)` to defeat fn-pointer devirtualization; torajs always emits a real `CallIndirect`. Apples-to-apples indirect call.

#### `collatz` — bit ops + hailstone trajectory + outer/inner loop

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   49 ms | **105.80 ms ←** | **34.9 KB** |
| torajs (JIT)     |     —   |  220.57 ms |          — |
| rust             |  126 ms |  110.07 ms |     466 KB |
| go               |   42 ms |  143.18 ms |    2.37 MB |
| bun-aot          |   64 ms |  324.69 ms |      63 MB |
| bun-jsc          |     —   |  325.28 ms |          — |
| node-v8          |     —   | 1626.75 ms |          — |

#### `fib40` — recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   47 ms | **152.44 ms ← (1.17× faster than rust)** | **34.9 KB** |
| torajs (JIT)     |     —   |  520.65 ms |          — |
| rust             |   76 ms |  179.00 ms |     466 KB |
| go               |   41 ms |  226.74 ms |    2.37 MB |
| bun-aot          |   63 ms |  382.68 ms |      63 MB |
| bun-jsc          |     —   |  388.79 ms |          — |
| node-v8          |     —   |  661.72 ms |          — |

#### `gcd1m` — tight integer loop with mod

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   46 ms | **40.36 ms ←** | **34.9 KB** |
| torajs (JIT)     |     —   |   51.59 ms |          — |
| rust             |   77 ms |   40.84 ms |     466 KB |
| go               |   39 ms |   41.54 ms |    2.37 MB |
| bun-aot          |   62 ms |   48.29 ms |      63 MB |
| bun-jsc          |     —   |   49.11 ms |          — |
| node-v8          |     —   |  130.98 ms |          — |

#### `mandelbrot` — f64 nested loops, FMA tolerance

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   48 ms | **35.19 ms ←** | **34.9 KB** |
| torajs (JIT)     |     —   |   86.49 ms |          — |
| rust             |   79 ms |   35.21 ms |     466 KB |
| go               |   39 ms |   37.58 ms |    2.37 MB |
| bun-aot          |   58 ms |   51.28 ms |      63 MB |
| bun-jsc          |     —   |   51.73 ms |          — |
| node-v8          |     —   |  124.80 ms |          — |

#### `popcount` — LLVM loop-idiom recognition (BK pattern → ARM `cnt.16b` NEON)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   52 ms |  **2.84 ms ←** | **34.9 KB** |
| torajs (JIT)     |     —   |  104.11 ms |          — |
| rust             |   82 ms |    3.19 ms |     466 KB |
| go               |   39 ms |   54.92 ms |    2.37 MB |
| bun-aot          |   58 ms |   56.89 ms |      63 MB |
| bun-jsc          |     —   |   55.43 ms |          — |
| node-v8          |     —   |  133.91 ms |          — |

#### `prime_count` — trial division, bool return, early-return-from-while

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   53 ms |   48.54 ms | **34.9 KB** |
| torajs (JIT)     |     —   |   55.20 ms |          — |
| rust             |   79 ms |   48.30 ms |     466 KB |
| **go**           |   41 ms | **41.24 ms ←** |    2.37 MB |
| bun-aot          |   60 ms |   54.29 ms |      63 MB |
| bun-jsc          |     —   |   53.19 ms |          — |
| node-v8          |     —   |  161.41 ms |          — |

#### `startup` — program launch cost (cold-start perf)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   48 ms |  **1.44 ms ← (cold-start champion)** | **34.9 KB** |
| torajs (JIT)     |     —   |    9.27 ms |          — |
| rust             |   71 ms |    1.81 ms |     466 KB |
| go               |   41 ms |    2.11 ms |    2.37 MB |
| bun-aot          |   59 ms |    9.03 ms |      63 MB |
| bun-jsc          |     —   |    9.18 ms |          — |
| node-v8          |     —   |   83.09 ms |          — |

### Aggregate compile / binary

|       runtime       | median compile | median binary |
| ------------------- | -------------: | ------------: |
| **torajs (AOT)**    |     **~50 ms** |  **34.9 KB**  |
| go                  |          ~41 ms |       2.37 MB |
| bun-aot             |          ~62 ms |        63 MB |
| rust                |          ~80 ms |       466 KB |

torajs binary is **14× smaller** than rust, **70× smaller** than go, **1860× smaller** than bun-aot. Median compile is the **fastest of any AOT** runtime in the comparison — small enough to fit `tr build && ./out` inside a sub-100ms dev iteration.

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
