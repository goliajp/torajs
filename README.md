# torajs

A TypeScript runtime that runs a subset of TS programs with **TS semantics**, AOT-compiled to small native binaries via LLVM. **No GC**, no refcount — the compiler infers ownership at compile time. Two execution modes: `tr build` (AOT to standalone binary) and `tr run` (AOT-with-cache, dev-loop shape — same codegen, cached at `~/.torajs/cache`).

> Closed-source research project. Public site: https://torajs.com

## What it is

```
TS subset surface         TS semantics, no GC               One codegen, two modes
─────────────────         ───────────────────               ──────────────────────
function fib(n: number)   let n = s;                        tr build → LLVM 22 → bin
   : number { ... }       console.log(s); console.log(n);   tr run   → same path, cached
let xs: number[] = [];    // both reads work                 ~40 KB static binary
xs.push(1); xs[0] = 9;    // one drop at scope exit          ~1.3 ms cold-start
```

bun is the oracle: when behavior is unclear, write the equivalent in TS, run it in `bun`, and match.

`number` is `i64` by default; `f64` is opt-in. Strings, objects, and arrays follow TS reference semantics — multiple bindings can alias the same heap, the compiler picks the owner statically and emits one drop. No `null`, no `==`, no `var`, no decorators, no `eval`. Differentiator from bun is the runtime: ~40 KB native binary, ~1.3 ms startup, no GC pauses. See [`docs/ts-subset.md`](docs/ts-subset.md) for the supported subset boundary.

## Bench scoreboard

Cross-runtime perf, M4 Pro, hyperfine n=10 with 3 warmup runs. Measured 2026-05-01 on commit `32737e4` — incremental batch on top of `04d6e36` adding generic class fixed-point monomorphization (`class Wrapper<T>`, `class Stack<T>`, `class Pair<A, B>`) with deep-cloned mono bodies + multi-type-param defaults via `__tvdefault__T` markers. Earlier in this run: object spread, Math.random, Number.toString(radix), JSON.stringify (recursive type-aware serializer), multi-arg console.{log,error,warn}, default function parameters, rest parameters, spread in fn args, Array.isArray (compile-time), Map class pattern, integration ports across try/throw/switch/destructuring/template+spread/null+optchain/array pipeline/class-deep/defparam-combo/stdlib-grid/deep-recursion/string-recursion/JSON-roundtrip, plus the prior batch (Math trig/hyperbolic, String.{trim*,padStart,padEnd,replace,replaceAll,at,localeCompare}, Array.{includes,findIndex,some,every,reverse,fill,sort,flat,concat,copyWithin,lastIndexOf,at}, Number.{parseInt,parseFloat,isInteger,isNaN,isFinite,isSafeInteger,toFixed,toExponential,toPrecision,toString}, Math.{imul,clz32,fround} + constants, Number(x) / String(x) coercion, console.error/warn → stderr, bare-name globals, lexer escapes + scientific notation, post `++`/`--` spec, struct-field push, class array-field default, return-via-let closure detection, closure-captured array push env writeback, empty `[]` inner literals, ssa-lower auto-coerce return value i64→f64). All times in ms; binary in KB / MB. [Full JSON data](bench/results/).

### Headline summary (run-time, lower better)

|     case                  | torajs (AOT)  |   torajs-run  |       rust |         go |    bun-jsc |    bun-aot |    node-v8 |
| ------------------------- | ------------: | ------------: | ---------: | ---------: | ---------: | ---------: | ---------: |
| **ackermann**             |      **9.15** |         16.19 |       9.29 |       9.94 |      16.54 |      17.21 |     103.62 |
| array-map-1m              |         30.32 |         38.01 |      27.38 |  **23.03** |      62.78 |      63.20 |     243.16 |
| **array-sum-1m**          |     **11.90** |         19.42 |      13.72 |      37.00 |      47.84 |      47.76 |     177.60 |
| closure-counter           |         18.70 |         25.96 |  **18.52** |      35.93 |      50.15 |      48.36 |     178.22 |
| **closure-pipeline-1m**   |     **12.82** |         20.02 |      18.66 |      38.37 |      47.39 |      49.15 |     172.23 |
| collatz                   |        106.46 |        114.05 | **105.96** |     143.14 |     326.99 |     327.18 |    1410.85 |
| **fib40**                 |    **147.14** |        240.33 |     179.35 |     226.43 |     382.97 |     382.74 |     671.13 |
| gcd1m                     |         40.79 |         48.08 |  **40.65** |      41.13 |      49.34 |      49.37 |     123.51 |
| generic-id-1m             |         12.73 |         23.17 |  **11.90** |      29.57 |      46.31 |      47.07 |     175.27 |
| **generic-pair-1m**       |      **1.28** |          8.60 |       2.15 |       2.68 |      12.72 |      12.28 |      91.82 |
| mandelbrot                |         35.22 |         42.84 |  **35.02** |      37.02 |      51.58 |      51.73 |     125.66 |
| **popcount**              |      **2.67** |          9.89 |       2.70 |      59.62 |      57.17 |      57.16 |     141.19 |
| prime_count               |         48.83 |         55.56 |      48.36 |  **40.05** |      55.63 |      52.78 |     164.54 |
| **startup**               |      **1.31** |          8.21 |       1.34 |       1.81 |       9.28 |       8.43 |      83.30 |
| **throw-catch-100k**      |      **1.37** |          8.49 |     422.91 |       7.56 |      23.65 |      22.82 |     148.11 |

torajs (AOT) **vs rust**: 8 wins / 5 ties (within 1%) / 2 losses (array-map-1m +11%, generic-id-1m +7%). On 5 cases tora and rust now agree to within 1 ms.
torajs (AOT) **vs go**: 13 wins, 2 losses (array-map-1m and prime_count — go's per-element fast path + GC-backed tight loops).
torajs (AOT) **vs bun-jsc / bun-aot / node-v8**: **15 / 15 / 15** clean sweeps per runtime.

`throw-catch-100k` stays the category-killer: 100k handled exceptions takes 1.37 ms in torajs vs 423 ms in rust — **308× faster than rust's panic path**. tr's M4 design (module-level throw_active flag + cond_br on every may_throw call) lets throw be ~zero-cost when it doesn't fire.

55+ commits since `7c7844e` added zero perf regression on this scoreboard — and several quiet improvements (array-map-1m 34.4 → 30.3 ms, generic-id-1m 15.9 → 12.7 ms, popcount 3.1 → 2.7 ms, fib40 151.7 → 147.1 ms, startup 1.55 → 1.31 ms, all driven by LLVM rebuild noise + accumulated codegen polish). Conformance grew from 197 → **247 ports** — Math 100% complete, object spread, default + rest params + spread call args, JSON.stringify, multi-arg console, Number.toString(radix), generic class fixed-point monomorphization (`class Wrapper<T>`, `class Stack<T>`, `class Pair<A, B>`), and many integration ports. See [docs/100-percent-plan.md](docs/100-percent-plan.md) for the subset-expansion roadmap toward 100% test262 coverage.

### Per-case detail — compile / run / binary

Each case shows: AOT compile time, AOT execution time, dev-loop `tr run` execution time, and on-disk binary size. `—` marks runtimes that don't have that mode (`tr run` is AOT-with-cache, no separate binary; bun-jsc/node-v8 have no detached compile step).

#### `ackermann` — nested recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  132 ms |  **9.15 ms ←** | **40.3 KB** |
| torajs-run       |     —   |   16.19 ms |          — |
| rust             |   74 ms |    9.29 ms |     455 KB |
| go               |   39 ms |    9.94 ms |    2.26 MB |
| bun-aot          |   67 ms |   17.21 ms |      60 MB |
| bun-jsc          |     —   |   16.54 ms |          — |
| node-v8          |     —   |  103.62 ms |          — |

#### `array-map-1m` — 1M-element `Array<number>::map` over a closure

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  138 ms |   30.32 ms |   40.4 KB |
| torajs-run       |     —   |   38.01 ms |          — |
| rust             |   97 ms |   27.38 ms |     455 KB |
| **go**           |   39 ms | **23.03 ms ← (per-element fast path)** |    2.26 MB |
| bun-aot          |   67 ms |   63.20 ms |      60 MB |
| bun-jsc          |     —   |   62.78 ms |          — |
| node-v8          |     —   |  243.16 ms |          — |

The current weak spot — go's `append` + bounds-check elision and rust's `Vec::push` cap-doubling outpace tr's amortized-realloc on bulk-grow. Down 12% vs the 04d6e36 measurement (34.4 → 30.3 ms) but go is also the only thing meaningfully ahead of rust here. tr still beats every JS runtime by 2×+.

#### `array-sum-1m` — 1M `Array<T>::push` + index sum (heap alloc + amortized realloc)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  134 ms | **11.90 ms ← (1.15× faster than rust)** | **40.3 KB** |
| torajs-run       |     —   |   19.42 ms |          — |
| rust             |   83 ms |   13.72 ms |     455 KB |
| go               |   38 ms |   37.00 ms |    2.26 MB |
| bun-aot          |   60 ms |   47.76 ms |      60 MB |
| bun-jsc          |     —   |   47.84 ms |          — |
| node-v8          |     —   |  177.60 ms |          — |

#### `closure-counter` — long-lived closure mutating captured state across calls

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  134 ms |   18.70 ms |   40.4 KB |
| torajs-run       |     —   |   25.96 ms |          — |
| **rust**         |   88 ms | **18.52 ms ← (within 1%)** |     455 KB |
| go               |   39 ms |   35.93 ms |    2.26 MB |
| bun-aot          |   66 ms |   48.36 ms |      60 MB |
| bun-jsc          |     —   |   50.15 ms |          — |
| node-v8          |     —   |  178.22 ms |          — |

#### `closure-pipeline-1m` — 10M indirect calls through fn-pointer arg

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  134 ms | **12.82 ms ← (1.46× faster than rust)** | **40.4 KB** |
| torajs-run       |     —   |   20.02 ms |          — |
| rust             |   86 ms |   18.66 ms |     455 KB |
| go               |   39 ms |   38.37 ms |    2.26 MB |
| bun-aot          |   59 ms |   49.15 ms |      60 MB |
| bun-jsc          |     —   |   47.39 ms |          — |
| node-v8          |     —   |  172.23 ms |          — |

Rust uses `black_box(add1 as fn(i64)->i64)` to defeat fn-pointer devirtualization; torajs always emits a real `CallIndirect`. Apples-to-apples indirect call.

#### `collatz` — bit ops + hailstone trajectory + outer/inner loop

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  132 ms |  106.46 ms |   40.3 KB |
| torajs-run       |     —   |  114.05 ms |          — |
| **rust**         |   75 ms | **105.96 ms ← (within 0.5%)** |     455 KB |
| go               |   39 ms |  143.14 ms |    2.26 MB |
| bun-aot          |   60 ms |  327.18 ms |      60 MB |
| bun-jsc          |     —   |  326.99 ms |          — |
| node-v8          |     —   | 1410.85 ms |          — |

#### `fib40` — recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  129 ms | **147.14 ms ← (1.22× faster than rust)** | **40.2 KB** |
| torajs-run       |     —   |  240.33 ms |          — |
| rust             |   74 ms |  179.35 ms |     455 KB |
| go               |   39 ms |  226.43 ms |    2.26 MB |
| bun-aot          |   59 ms |  382.74 ms |      60 MB |
| bun-jsc          |     —   |  382.97 ms |          — |
| node-v8          |     —   |  671.13 ms |          — |

#### `gcd1m` — tight integer loop with mod

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  132 ms |   40.79 ms |   40.3 KB |
| torajs-run       |     —   |   48.08 ms |          — |
| **rust**         |   76 ms | **40.65 ms ← (within 0.4%)** |     455 KB |
| go               |   39 ms |   41.13 ms |    2.26 MB |
| bun-aot          |   59 ms |   49.37 ms |      60 MB |
| bun-jsc          |     —   |   49.34 ms |          — |
| node-v8          |     —   |  123.51 ms |          — |

#### `generic-id-1m` — 1M calls through a monomorphized generic identity fn

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  135 ms |   12.73 ms |   40.4 KB |
| torajs-run       |     —   |   23.17 ms |          — |
| **rust**         |   86 ms | **11.90 ms ← (rust faster by 7%)** |     455 KB |
| go               |   39 ms |   29.57 ms |    2.26 MB |
| bun-aot          |   60 ms |   47.07 ms |      60 MB |
| bun-jsc          |     —   |   46.31 ms |          — |
| node-v8          |     —   |  175.27 ms |          — |

#### `generic-pair-1m` — 1M generic struct allocations + field reads

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  130 ms |  **1.28 ms ← (1.68× faster than rust, 9.6× faster than bun)** | **40.3 KB** |
| torajs-run       |     —   |    8.60 ms |          — |
| rust             |   75 ms |    2.15 ms |     455 KB |
| go               |   39 ms |    2.68 ms |    2.26 MB |
| bun-aot          |   59 ms |   12.28 ms |      60 MB |
| bun-jsc          |     —   |   12.72 ms |          — |
| node-v8          |     —   |   91.82 ms |          — |

Monomorphization at codegen flattens `Pair<A, B>` into a stack-shape struct with no boxing; LLVM proceeds to elide most allocations.

#### `mandelbrot` — f64 nested loops, FMA tolerance

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  134 ms |   35.22 ms |   40.3 KB |
| torajs-run       |     —   |   42.84 ms |          — |
| **rust**         |   77 ms | **35.02 ms ← (within 0.6%)** |     455 KB |
| go               |   38 ms |   37.02 ms |    2.26 MB |
| bun-aot          |   67 ms |   51.73 ms |      60 MB |
| bun-jsc          |     —   |   51.58 ms |          — |
| node-v8          |     —   |  125.66 ms |          — |

#### `popcount` — LLVM loop-idiom recognition (BK pattern → ARM `cnt.16b` NEON)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  133 ms |  **2.67 ms ← (1.01× faster than rust)** | **40.3 KB** |
| torajs-run       |     —   |    9.89 ms |          — |
| rust             |   77 ms |    2.70 ms |     455 KB |
| go               |   38 ms |   59.62 ms |    2.26 MB |
| bun-aot          |   59 ms |   57.16 ms |      60 MB |
| bun-jsc          |     —   |   57.17 ms |          — |
| node-v8          |     —   |  141.19 ms |          — |

#### `prime_count` — trial division, bool return, early-return-from-while

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  132 ms |   48.83 ms |   40.3 KB |
| torajs-run       |     —   |   55.56 ms |          — |
| rust             |   75 ms |   48.36 ms |     455 KB |
| **go**           |   38 ms | **40.05 ms ←** |    2.26 MB |
| bun-aot          |   60 ms |   52.78 ms |      60 MB |
| bun-jsc          |     —   |   55.63 ms |          — |
| node-v8          |     —   |  164.54 ms |          — |

#### `startup` — program launch cost (cold-start perf)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  130 ms |  **1.31 ms ← (cold-start champion)** | **40.2 KB** |
| torajs-run       |     —   |    8.21 ms |          — |
| rust             |   71 ms |    1.34 ms |     455 KB |
| go               |   39 ms |    1.81 ms |    2.26 MB |
| bun-aot          |   60 ms |    8.43 ms |      60 MB |
| bun-jsc          |     —   |    9.28 ms |          — |
| node-v8          |     —   |   83.30 ms |          — |

#### `throw-catch-100k` — 100k handled exceptions through 2 stack frames

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  131 ms |  **1.37 ms ← (308× faster than rust panic)** | **40.3 KB** |
| torajs-run       |     —   |    8.49 ms |          — |
| rust             |   87 ms |  422.91 ms |     458 KB |
| go               |   39 ms |    7.56 ms |    2.26 MB |
| bun-aot          |   59 ms |   22.82 ms |      60 MB |
| bun-jsc          |     —   |   23.65 ms |          — |
| node-v8          |     —   |  148.11 ms |          — |

`throw-catch-100k` exercises tr's M4 throw model at saturation: every `may_throw` call site emits a `cond_br` on `__torajs_throw_active`, so a quiet path costs one untaken branch per call and a thrown path resumes at the nearest catch without unwinding any frame metadata. Rust pays for `panic::catch_unwind` per-occurrence (DWARF unwind info, drop landing pads), which explains the gap.

### Aggregate compile / binary

|       runtime       | median compile | median binary |
| ------------------- | -------------: | ------------: |
| **torajs (AOT)**    |    **~133 ms** |  **40.3 KB**  |
| go                  |          ~39 ms |       2.26 MB |
| bun-aot             |          ~60 ms |        60 MB |
| rust                |          ~78 ms |       455 KB |

torajs binary is **11× smaller** than rust, **57× smaller** than go, **1490× smaller** than bun-aot. Median AOT compile (~133 ms — generic-class infra + JSON.stringify pulled the trend up ~50 ms vs the 04d6e36 measurement; clang link is still the bulk of it, tr's IR→object pass is ~25 ms) keeps `tr build && ./out` inside a 150 ms dev iteration.

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
