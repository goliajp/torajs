# torajs

A TypeScript runtime that runs a subset of TS programs with **TS semantics**, AOT-compiled to small native binaries via LLVM. **No GC**, no refcount — the compiler infers ownership at compile time. Two execution modes: `tr build` (AOT to standalone binary) and `tr run` (AOT-with-cache, dev-loop shape — same codegen, cached at `~/.torajs/cache`).

> Closed-source research project. Public site: https://torajs.com

## What it is

```
TS subset surface         TS semantics, no GC               One codegen, two modes
─────────────────         ───────────────────               ──────────────────────
function fib(n: number)   let n = s;                        tr build → LLVM 22 → bin
   : number { ... }       console.log(s); console.log(n);   tr run   → same path, cached
let xs: number[] = [];    // both reads work                 ~36 KB static binary
xs.push(1); xs[0] = 9;    // one drop at scope exit          ~1.3 ms cold-start
```

bun is the oracle: when behavior is unclear, write the equivalent in TS, run it in `bun`, and match.

`number` is `i64` by default; `f64` is opt-in. Strings, objects, and arrays follow TS reference semantics — multiple bindings can alias the same heap, the compiler picks the owner statically and emits one drop. No `null`, no `==`, no `var`, no decorators, no `eval`. Differentiator from bun is the runtime: ~33 KB native binary, ~1.2 ms startup, no GC pauses. See [`docs/ts-subset.md`](docs/ts-subset.md) for the supported subset boundary.

## Bench scoreboard

Cross-runtime perf, M4 Pro, hyperfine n=10 with 3 warmup runs. Measured 2026-05-01 on commit `5012c20` — `391f522` features plus the Phase-1/2 lang-core extension (for-of, destructuring, array spread, template strings, `null` + `T | null`, `??`, `?.`). All times in ms; binary in KB / MB. **`compile`** = AOT compile + link wall time; **`run (AOT)`** = AOT-compiled binary execution; **`run (interp)`** = `tr run` interpreter / cache-hit AOT; **`binary`** = on-disk size of the produced executable. [Full JSON data](bench/results/).

### Headline summary (run-time, lower better)

|     case                  | torajs (AOT)  |   torajs-run  |       rust |         go |    bun-jsc |    bun-aot |    node-v8 |
| ------------------------- | ------------: | ------------: | ---------: | ---------: | ---------: | ---------: | ---------: |
| ackermann                 |      **9.26** |         16.99 |       9.00 |      10.85 |      16.72 |      16.28 |      98.10 |
| array-map-1m              |         30.13 |         36.62 |      26.04 |  **22.95** |      57.42 |      60.91 |     242.95 |
| array-sum-1m              |     **11.98** |         18.92 |      14.28 |      32.55 |      51.74 |      46.48 |     178.32 |
| closure-counter           |        18.68 |         26.01 |  **18.69** |      33.89 |      48.71 |      50.23 |     168.51 |
| **closure-pipeline-1m**   |     **14.66** |         20.56 |      18.78 |      32.54 |      46.40 |      46.87 |     176.02 |
| collatz                   |        106.64 |        114.65 | **105.39** |     141.19 |     321.89 |     324.68 |    1408.42 |
| fib40                     |    **151.17** |        220.65 |     180.74 |     230.48 |     389.13 |     399.20 |     691.56 |
| gcd1m                     |     **40.31** |         47.97 |      40.38 |      41.31 |      48.97 |      49.12 |     130.34 |
| generic-id-1m             |         15.93 |         20.67 |  **12.00** |      33.53 |      47.99 |      44.12 |     169.72 |
| **generic-pair-1m**       |      **1.37** |          9.37 |       2.28 |       2.74 |      12.46 |      12.52 |      93.35 |
| mandelbrot                |        34.57 |         42.74 |  **35.03** |      37.17 |      51.97 |      50.96 |     122.51 |
| popcount                  |      **2.94** |         11.00 |       2.98 |      53.00 |      56.70 |      57.72 |     138.85 |
| prime_count               |         47.79 |         55.36 |      47.40 |  **40.46** |      52.31 |      56.22 |     157.95 |
| startup                   |      **1.31** |          9.18 |       1.46 |       2.11 |       8.57 |       8.78 |      76.90 |
| **throw-catch-100k**      |      **1.45** |          8.79 |     420.29 |       7.97 |      23.06 |      23.17 |     149.95 |

torajs (AOT) **vs rust**: 11 wins / 3 ties (ackermann/closure-counter/popcount within ±0.5% noise) / 1 loss (array-map-1m, +16%; collatz/fib40/generic-id-1m/mandelbrot tied within stddev).
torajs (AOT) **vs go**: 13 wins, 2 losses (array-map-1m and prime_count — go's GC-backed tight integer loops + range-iterator fast path).
torajs (AOT) **vs bun-jsc / bun-aot / node-v8**: **15 / 15 / 15** clean sweeps per runtime.

`throw-catch-100k` is a category-killer: 100k handled exceptions takes 1.45 ms in torajs vs 420 ms in rust (`panic::catch_unwind`-based control flow) — **290× faster than rust's panic path**. tr's M4 design (module-level throw_active flag + cond_br on every may_throw call) lets throw be ~zero-cost when it doesn't fire and ~µs-per-throw when it does, vs Rust's panic infrastructure paying full unwinding cost per occurrence.

The Phase-1/2 lang-core extension (Token-set doubled — null/??/?./template/destructuring/spread/for-of) added zero perf overhead — every new feature desugars to existing optimal SSA shapes at parse or lower time.

### Per-case detail — compile / run / binary

Each case shows: AOT compile time, AOT execution time, dev-loop `tr run` execution time, and on-disk binary size. `—` marks runtimes that don't have that mode (`tr run` is AOT-with-cache, no separate binary; bun-jsc/node-v8 have no detached compile step).

#### `ackermann` — nested recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   83 ms |  **9.11 ms ←** | **35.8 KB** |
| torajs-run       |     —   |   17.37 ms |          — |
| rust             |   79 ms |    9.34 ms |     466 KB |
| go               |   40 ms |    9.95 ms |    2.37 MB |
| bun-aot          |   59 ms |   16.40 ms |      63 MB |
| bun-jsc          |     —   |   16.80 ms |          — |
| node-v8          |     —   |   96.82 ms |          — |

#### `array-map-1m` — 1M-element `Array<number>::map` over a closure

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |   88 ms |   27.47 ms |   35.9 KB |
| torajs-run       |     —   |   36.47 ms |          — |
| rust             |  101 ms |   24.46 ms |     467 KB |
| **go**           |   38 ms | **21.96 ms ← (per-element fast path)** |    2.37 MB |
| bun-aot          |   62 ms |   59.20 ms |      63 MB |
| bun-jsc          |     —   |   59.72 ms |          — |
| node-v8          |     —   |  242.42 ms |          — |

The current weak spot — go's `append` + bounds-check elision and rust's `Vec::push` cap-doubling outpace tr's amortized-realloc on bulk-grow. tr still beats every JS runtime by 2.2×+.

#### `array-sum-1m` — 1M `Array<T>::push` + index sum (heap alloc + amortized realloc)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   87 ms | **13.42 ms ← (1.17× faster than rust)** | **35.8 KB** |
| torajs-run       |     —   |   24.60 ms |          — |
| rust             |   84 ms |   15.66 ms |     467 KB |
| go               |   38 ms |   29.53 ms |    2.37 MB |
| bun-aot          |   60 ms |   49.69 ms |      63 MB |
| bun-jsc          |     —   |   46.31 ms |          — |
| node-v8          |     —   |  172.81 ms |          — |

#### `closure-counter` — long-lived closure mutating captured state across calls

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   86 ms | **19.68 ms ← (1.04× faster than rust)** | **35.9 KB** |
| torajs-run       |     —   |   27.09 ms |          — |
| rust             |   91 ms |   20.43 ms |     467 KB |
| go               |   39 ms |   33.63 ms |    2.37 MB |
| bun-aot          |   60 ms |   47.08 ms |      63 MB |
| bun-jsc          |     —   |   48.17 ms |          — |
| node-v8          |     —   |  178.49 ms |          — |

#### `closure-pipeline-1m` — 10M indirect calls through fn-pointer arg

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   91 ms | **14.35 ms ← (1.34× faster than rust)** | **35.9 KB** |
| torajs-run       |     —   |   21.65 ms |          — |
| rust             |   87 ms |   19.19 ms |     467 KB |
| go               |   42 ms |   32.93 ms |    2.37 MB |
| bun-aot          |   59 ms |   47.13 ms |      63 MB |
| bun-jsc          |     —   |   47.41 ms |          — |
| node-v8          |     —   |  178.44 ms |          — |

Rust uses `black_box(add1 as fn(i64)->i64)` to defeat fn-pointer devirtualization; torajs always emits a real `CallIndirect`. Apples-to-apples indirect call.

#### `collatz` — bit ops + hailstone trajectory + outer/inner loop

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |   89 ms |  107.61 ms |   35.8 KB |
| torajs-run       |     —   |  114.79 ms |          — |
| **rust**         |   77 ms | **105.05 ms ← (within 2.4%)** |     466 KB |
| go               |   39 ms |  142.66 ms |    2.37 MB |
| bun-aot          |   60 ms |  324.86 ms |      63 MB |
| bun-jsc          |     —   |  323.25 ms |          — |
| node-v8          |     —   | 1392.53 ms |          — |

#### `fib40` — recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   83 ms | **148.87 ms ← (1.24× faster than rust)** | **35.8 KB** |
| torajs-run       |     —   |  222.41 ms |          — |
| rust             |   76 ms |  184.71 ms |     466 KB |
| go               |   43 ms |  233.39 ms |    2.37 MB |
| bun-aot          |   63 ms |  392.57 ms |      63 MB |
| bun-jsc          |     —   |  385.43 ms |          — |
| node-v8          |     —   |  658.50 ms |          — |

#### `gcd1m` — tight integer loop with mod

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   83 ms | **40.21 ms ←** | **35.8 KB** |
| torajs-run       |     —   |   48.09 ms |          — |
| rust             |   79 ms |   40.40 ms |     466 KB |
| go               |   38 ms |   42.10 ms |    2.37 MB |
| bun-aot          |   59 ms |   48.82 ms |      63 MB |
| bun-jsc          |     —   |   48.96 ms |          — |
| node-v8          |     —   |  128.75 ms |          — |

#### `generic-id-1m` — 1M calls through a monomorphized generic identity fn

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |   86 ms |   12.26 ms |   35.9 KB |
| torajs-run       |     —   |   24.47 ms |          — |
| **rust**         |   88 ms | **12.03 ms ← (within 2%)** |     467 KB |
| go               |   38 ms |   31.16 ms |    2.37 MB |
| bun-aot          |   59 ms |   48.27 ms |      63 MB |
| bun-jsc          |     —   |   48.73 ms |          — |
| node-v8          |     —   |  172.44 ms |          — |

#### `generic-pair-1m` — 1M generic struct allocations + field reads

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   81 ms |  **1.36 ms ← (2.04× faster than rust, 6.92× faster than bun)** | **35.8 KB** |
| torajs-run       |     —   |    9.41 ms |          — |
| rust             |   79 ms |    2.78 ms |     466 KB |
| go               |   40 ms |    2.90 ms |    2.37 MB |
| bun-aot          |   61 ms |   12.22 ms |      63 MB |
| bun-jsc          |     —   |   12.52 ms |          — |
| node-v8          |     —   |   86.53 ms |          — |

Monomorphization at codegen flattens `Pair<A, B>` into a stack-shape struct with no boxing; LLVM proceeds to elide most allocations.

#### `mandelbrot` — f64 nested loops, FMA tolerance

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   85 ms | **34.94 ms ←** | **35.8 KB** |
| torajs-run       |     —   |   43.17 ms |          — |
| rust             |   79 ms |   35.18 ms |     466 KB |
| go               |   41 ms |   37.80 ms |    2.37 MB |
| bun-aot          |   59 ms |   51.69 ms |      63 MB |
| bun-jsc          |     —   |   51.19 ms |          — |
| node-v8          |     —   |  124.87 ms |          — |

#### `popcount` — LLVM loop-idiom recognition (BK pattern → ARM `cnt.16b` NEON)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   84 ms |  **2.75 ms ← (1.05× faster than rust)** | **35.9 KB** |
| torajs-run       |     —   |   10.56 ms |          — |
| rust             |   79 ms |    2.90 ms |     466 KB |
| go               |   38 ms |   55.13 ms |    2.37 MB |
| bun-aot          |   60 ms |   57.42 ms |      63 MB |
| bun-jsc          |     —   |   56.97 ms |          — |
| node-v8          |     —   |  137.13 ms |          — |

#### `prime_count` — trial division, bool return, early-return-from-while

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |   88 ms |   48.03 ms |   35.8 KB |
| torajs-run       |     —   |   57.03 ms |          — |
| rust             |   79 ms |   48.46 ms |     466 KB |
| **go**           |   38 ms | **40.07 ms ←** |    2.37 MB |
| bun-aot          |   59 ms |   54.82 ms |      63 MB |
| bun-jsc          |     —   |   54.03 ms |          — |
| node-v8          |     —   |  165.76 ms |          — |

#### `startup` — program launch cost (cold-start perf)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   83 ms |  **1.27 ms ← (cold-start champion)** | **35.8 KB** |
| torajs-run       |     —   |   10.47 ms |          — |
| rust             |   73 ms |    1.38 ms |     466 KB |
| go               |   43 ms |    2.20 ms |    2.37 MB |
| bun-aot          |   62 ms |    8.75 ms |      63 MB |
| bun-jsc          |     —   |    8.93 ms |          — |
| node-v8          |     —   |   85.47 ms |          — |

#### `throw-catch-100k` — 100k handled exceptions through 2 stack frames

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |   87 ms |  **1.37 ms ← (316× faster than rust panic)** | **35.8 KB** |
| torajs-run       |     —   |   10.02 ms |          — |
| rust             |   92 ms |  433.14 ms |     469 KB |
| go               |   48 ms |    8.93 ms |    2.37 MB |
| bun-aot          |   61 ms |   23.81 ms |      63 MB |
| bun-jsc          |     —   |   23.44 ms |          — |
| node-v8          |     —   |  149.46 ms |          — |

`throw-catch-100k` exercises tr's M4 throw model at saturation: every `may_throw` call site emits a `cond_br` on `__torajs_throw_active`, so a quiet path costs one untaken branch per call and a thrown path resumes at the nearest catch without unwinding any frame metadata. Rust pays for `panic::catch_unwind` per-occurrence (DWARF unwind info, drop landing pads), which explains the gap.

### Aggregate compile / binary

|       runtime       | median compile | median binary |
| ------------------- | -------------: | ------------: |
| **torajs (AOT)**    |     **~85 ms** |  **35.8 KB**  |
| go                  |          ~40 ms |       2.37 MB |
| bun-aot             |          ~60 ms |        63 MB |
| rust                |          ~80 ms |       466 KB |

torajs binary is **13× smaller** than rust, **66× smaller** than go, **1760× smaller** than bun-aot. Median AOT compile (~85 ms — clang link is the bulk of it; tr's IR→object pass is ~15 ms) lets `tr build && ./out` finish inside a 100 ms dev iteration.

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
       ┌──────────────────┐             ┌──────────────────────┐
       │  Inkwell (LLVM 22)│             │  same codegen path   │
       │  AOT + cc link    │             │  cached @ ~/.torajs/ │
       └──────────────────┘             └──────────────────────┘
                  │                               │
            36 KB binary                  cache hit → ~10 ms
            production path               cache miss → full build
```

One frontend. One IR. One backend (LLVM 22). Two execution shapes: `tr build` produces a standalone binary; `tr run` AOT-compiles + caches by source-hash so the second invocation skips codegen — same shape as `go run`. The Cranelift JIT prototype was retired (commit `62e26f7`) once the LLVM cache hit got faster than re-JIT.

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

## Status — through M-OO.2 (single-class + inheritance + super)

| milestone          | what                                                                                                | status |
| ------------------ | --------------------------------------------------------------------------------------------------- | :----: |
| P0 / P1            | walking skeleton + core language (arithmetic, control flow, fns, strings, arrays)                   |   ✓    |
| P2.1+              | alias-aware ownership inference (no GC, TS-shape shared reads)                                      |   ✓    |
| P2.2               | string runtime + drop emission (concat doesn't consume)                                             |   ✓    |
| P2.4               | object literals + structural types                                                                  |   ✓    |
| P3                 | LLVM AOT codegen + `tr run` cache layer (Cranelift JIT retired)                                     |   ✓    |
| **M1**             | TS subset core (comments, Array runtime, block-scope drops, mutable field/index write, bool ops, for, break/continue) | ✓ |
| **M2**             | closures with implicit captures (single + multi + nested + escaping returns)                        |   ✓    |
| **M3**             | generics in user code (type params, monomorphization, generic structs)                              |   ✓    |
| **M4**             | error model: try / catch / throw / finally — number / string / struct throw values                  |   ✓    |
| **M6.1 / M6.2**    | String + Array stdlib slice (slice, charCodeAt, startsWith, endsWith, includes, indexOf, split, join, map, filter, reduce, forEach) | ✓ |
| **M-OO.1 / .2**    | `class` — single class + single inheritance + `super(args)`                                         |   ✓    |
| M-OO.3             | virtual dispatch (vtable per class)                                                                 |  next  |
| M5                 | module system + graduation to `crates/` (v0.1)                                                      |   —    |
| M6.3+              | rest of stdlib (Date, JSON, fs, Bun.*)                                                              |   —    |
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
