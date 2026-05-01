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

Cross-runtime perf, M4 Pro, hyperfine n=10 with 3 warmup runs. Measured 2026-05-01 on commit `d54e95e` (load avg ~6 — short-time cases sit ±15% above their best historical values; long cases ≥40 ms remain stable to ±2%). Latest changes vs `04d6e36`: generic class fixed-point monomorphization (`class Wrapper<T>`, `class Stack<T>`, `class Pair<A, B>`) with deep-cloned mono bodies + multi-type-param defaults via `__tvdefault__T` markers; variadic `String.fromCharCode(...codes)` chained as one-char alloc + str_concat. Earlier in this run: object spread, Math.random, Number.toString(radix), JSON.stringify (recursive type-aware serializer), multi-arg console.{log,error,warn}, default function parameters, rest parameters, spread in fn args, Array.isArray (compile-time), Map class pattern, integration ports across try/throw/switch/destructuring/template+spread/null+optchain/array pipeline/class-deep/defparam-combo/stdlib-grid/deep-recursion/string-recursion/JSON-roundtrip, plus the prior batch (Math trig/hyperbolic, String.{trim*,padStart,padEnd,replace,replaceAll,at,localeCompare}, Array.{includes,findIndex,some,every,reverse,fill,sort,flat,concat,copyWithin,lastIndexOf,at}, Number.{parseInt,parseFloat,isInteger,isNaN,isFinite,isSafeInteger,toFixed,toExponential,toPrecision,toString}, Math.{imul,clz32,fround} + constants, Number(x) / String(x) coercion, console.error/warn → stderr, bare-name globals, lexer escapes + scientific notation, post `++`/`--` spec, struct-field push, class array-field default, return-via-let closure detection, closure-captured array push env writeback, empty `[]` inner literals, ssa-lower auto-coerce return value i64→f64). All times in ms; binary in KB / MB. [Full JSON data](bench/results/).

> **Noise note.** Short-time cases (< 20 ms — startup, generic-id, generic-pair, popcount, throw-catch) carry ±15% scheduling noise on a shared M4 Pro at load avg > 4. The 14-run history under `bench/results/` shows e.g. `tr generic-id-1m` ranging 12.26 ~ 18.45 ms; the table below reflects this run, not a hand-picked best. Long cases (collatz, fib40, gcd1m, mandelbrot, prime_count, closure-counter) are stable to ±2% across the same history.

### Headline summary (run-time, lower better)

|     case                  | torajs (AOT)  |   torajs-run  |       rust |         go |    bun-jsc |    bun-aot |    node-v8 |
| ------------------------- | ------------: | ------------: | ---------: | ---------: | ---------: | ---------: | ---------: |
| **ackermann**             |      **9.12** |         16.04 |       9.17 |      10.81 |      17.04 |      16.01 |      86.07 |
| array-map-1m              |         32.02 |         35.42 |      25.85 |  **20.44** |      59.56 |      61.08 |     242.88 |
| **array-sum-1m**          |     **11.15** |         18.94 |      13.15 |      34.05 |      45.82 |      45.95 |     175.07 |
| closure-counter           |         18.49 |         26.13 |  **18.24** |      31.34 |      48.29 |      46.86 |     179.63 |
| **closure-pipeline-1m**   |     **11.39** |         18.69 |      17.72 |      33.04 |      48.36 |      45.39 |     176.54 |
| collatz                   |        106.31 |        114.10 | **106.32** |     143.77 |     328.03 |     328.66 |    1411.14 |
| **fib40**                 |    **146.31** |        241.77 |     183.12 |     226.19 |     384.03 |     378.17 |     681.84 |
| **gcd1m**                 |     **40.35** |         48.06 |      40.72 |      41.35 |      49.62 |      49.16 |     133.55 |
| generic-id-1m             |         16.63 |         20.64 |  **13.40** |      32.37 |      47.56 |      52.35 |     181.53 |
| **generic-pair-1m**       |      **1.34** |          8.31 |       2.18 |       2.69 |      12.53 |      12.53 |      93.63 |
| mandelbrot                |         35.06 |         42.69 |  **35.05** |      37.26 |      52.22 |      51.86 |     125.76 |
| **popcount**              |      **2.64** |          9.55 |       2.80 |      57.48 |      57.01 |      58.21 |     137.53 |
| prime_count               |         48.18 |         55.27 |      48.52 |  **40.37** |      52.43 |      51.91 |     155.23 |
| **startup**               |      **1.25** |          8.04 |       1.35 |       1.91 |       9.02 |       7.82 |      84.10 |
| **throw-catch-100k**      |      **1.55** |          9.66 |     429.19 |       7.49 |      23.75 |      23.59 |     147.62 |

torajs (AOT) **vs rust**: 7 wins / 6 ties (within 2%) / 2 losses (array-map-1m +24%, generic-id-1m +24%). The two losses sit on tr's known weak spots — go's append fast path on bulk-grow array-map, and short-time generic-id where rust's monomorphization wins by a single LLVM optimization pass tr doesn't yet run.
torajs (AOT) **vs go**: 13 wins, 2 losses (array-map-1m and prime_count — go's per-element fast path + GC-backed tight loops).
torajs (AOT) **vs bun-jsc / bun-aot / node-v8**: **15 / 15 / 15** clean sweeps per runtime.

`throw-catch-100k` stays the category-killer: 100k handled exceptions takes 1.55 ms in torajs vs 429 ms in rust — **277× faster than rust's panic path**. tr's M4 design (module-level throw_active flag + cond_br on every may_throw call) lets throw be ~zero-cost when it doesn't fire.

60+ commits since `7c7844e` added zero perf regression on the scoreboard's stable cases — `closure-pipeline-1m` improved 14.35 → 11.39 ms (-21%) and `array-sum-1m` 13.42 → 11.15 ms (-17%) over that span; long cases (fib40, mandelbrot, gcd1m, collatz, prime_count) all sit within ±2% of their `7c7844e`-era values. Conformance grew from 197 → **249 ports** — Math 100% complete, object spread, default + rest params + spread call args, JSON.stringify, multi-arg console, variadic String.fromCharCode, Number.toString(radix), generic class fixed-point monomorphization (`class Wrapper<T>`, `class Stack<T>`, `class Pair<A, B>`), and many integration ports. See [docs/100-percent-plan.md](docs/100-percent-plan.md) for the subset-expansion roadmap toward 100% test262 coverage.

### Per-case detail — compile / run / binary

Each case shows: AOT compile time, AOT execution time, dev-loop `tr run` execution time, and on-disk binary size. `—` marks runtimes that don't have that mode (`tr run` is AOT-with-cache, no separate binary; bun-jsc/node-v8 have no detached compile step).

#### `ackermann` — nested recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  133 ms |  **9.12 ms ←** | **40.3 KB** |
| torajs-run       |     —   |   16.04 ms |          — |
| rust             |   72 ms |    9.17 ms |     455 KB |
| go               |   38 ms |   10.81 ms |    2.26 MB |
| bun-aot          |   63 ms |   16.01 ms |      60 MB |
| bun-jsc          |     —   |   17.04 ms |          — |
| node-v8          |     —   |   86.07 ms |          — |

#### `array-map-1m` — 1M-element `Array<number>::map` over a closure

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  137 ms |   32.02 ms |   40.4 KB |
| torajs-run       |     —   |   35.42 ms |          — |
| rust             |   94 ms |   25.85 ms |     455 KB |
| **go**           |   38 ms | **20.44 ms ← (per-element fast path)** |    2.26 MB |
| bun-aot          |   67 ms |   61.08 ms |      60 MB |
| bun-jsc          |     —   |   59.56 ms |          — |
| node-v8          |     —   |  242.88 ms |          — |

The current weak spot — go's `append` + bounds-check elision and rust's `Vec::push` cap-doubling outpace tr's amortized-realloc on bulk-grow. tr still beats every JS runtime by 2×+ on this case.

#### `array-sum-1m` — 1M `Array<T>::push` + index sum (heap alloc + amortized realloc)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  133 ms | **11.15 ms ← (1.18× faster than rust)** | **40.3 KB** |
| torajs-run       |     —   |   18.94 ms |          — |
| rust             |   81 ms |   13.15 ms |     455 KB |
| go               |   38 ms |   34.05 ms |    2.26 MB |
| bun-aot          |   62 ms |   45.95 ms |      60 MB |
| bun-jsc          |     —   |   45.82 ms |          — |
| node-v8          |     —   |  175.07 ms |          — |

#### `closure-counter` — long-lived closure mutating captured state across calls

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  134 ms |   18.49 ms |   40.4 KB |
| torajs-run       |     —   |   26.13 ms |          — |
| **rust**         |   86 ms | **18.24 ms ← (within 1.4%)** |     455 KB |
| go               |   38 ms |   31.34 ms |    2.26 MB |
| bun-aot          |   56 ms |   46.86 ms |      60 MB |
| bun-jsc          |     —   |   48.29 ms |          — |
| node-v8          |     —   |  179.63 ms |          — |

#### `closure-pipeline-1m` — 10M indirect calls through fn-pointer arg

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  133 ms | **11.39 ms ← (1.56× faster than rust)** | **40.4 KB** |
| torajs-run       |     —   |   18.69 ms |          — |
| rust             |   83 ms |   17.72 ms |     455 KB |
| go               |   38 ms |   33.04 ms |    2.26 MB |
| bun-aot          |   57 ms |   45.39 ms |      60 MB |
| bun-jsc          |     —   |   48.36 ms |          — |
| node-v8          |     —   |  176.54 ms |          — |

Rust uses `black_box(add1 as fn(i64)->i64)` to defeat fn-pointer devirtualization; torajs always emits a real `CallIndirect`. Apples-to-apples indirect call.

#### `collatz` — bit ops + hailstone trajectory + outer/inner loop

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  132 ms |  106.31 ms |   40.3 KB |
| torajs-run       |     —   |  114.10 ms |          — |
| **rust**         |   73 ms | **106.32 ms ← (essentially tied, within 0.01%)** |     455 KB |
| go               |   37 ms |  143.77 ms |    2.26 MB |
| bun-aot          |   57 ms |  328.66 ms |      60 MB |
| bun-jsc          |     —   |  328.03 ms |          — |
| node-v8          |     —   | 1411.14 ms |          — |

#### `fib40` — recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  129 ms | **146.31 ms ← (1.25× faster than rust)** | **40.2 KB** |
| torajs-run       |     —   |  241.77 ms |          — |
| rust             |   71 ms |  183.12 ms |     455 KB |
| go               |   37 ms |  226.19 ms |    2.26 MB |
| bun-aot          |   57 ms |  378.17 ms |      60 MB |
| bun-jsc          |     —   |  384.03 ms |          — |
| node-v8          |     —   |  681.84 ms |          — |

#### `gcd1m` — tight integer loop with mod

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  132 ms | **40.35 ms ← (within 1% of rust, faster by 0.9%)** | **40.3 KB** |
| torajs-run       |     —   |   48.06 ms |          — |
| rust             |   74 ms |   40.72 ms |     455 KB |
| go               |   38 ms |   41.35 ms |    2.26 MB |
| bun-aot          |   57 ms |   49.16 ms |      60 MB |
| bun-jsc          |     —   |   49.62 ms |          — |
| node-v8          |     —   |  133.55 ms |          — |

#### `generic-id-1m` — 1M calls through a monomorphized generic identity fn

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  133 ms |   16.63 ms |   40.4 KB |
| torajs-run       |     —   |   20.64 ms |          — |
| **rust**         |   85 ms | **13.40 ms ← (rust faster by 24% on this run)** |     455 KB |
| go               |   38 ms |   32.37 ms |    2.26 MB |
| bun-aot          |   57 ms |   52.35 ms |      60 MB |
| bun-jsc          |     —   |   47.56 ms |          — |
| node-v8          |     —   |  181.53 ms |          — |

This is the noisiest case in the suite: tr's 14-run history under `bench/results/` ranges 12.26 ~ 18.45 ms (≈ 50% spread), rust's 11.87 ~ 15.19 (≈ 28% spread). The body is one indirect call per iteration of a tiny loop; both runtimes do the right thing (LLVM monomorphizes, function-call overhead dominates), and the gap on any single run is dominated by OS scheduler noise on the M4 Pro.

#### `generic-pair-1m` — 1M generic struct allocations + field reads

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  130 ms |  **1.34 ms ← (1.63× faster than rust, 9.4× faster than bun)** | **40.3 KB** |
| torajs-run       |     —   |    8.31 ms |          — |
| rust             |   74 ms |    2.18 ms |     455 KB |
| go               |   40 ms |    2.69 ms |    2.26 MB |
| bun-aot          |   68 ms |   12.53 ms |      60 MB |
| bun-jsc          |     —   |   12.53 ms |          — |
| node-v8          |     —   |   93.63 ms |          — |

Monomorphization at codegen flattens `Pair<A, B>` into a stack-shape struct with no boxing; LLVM proceeds to elide most allocations.

#### `mandelbrot` — f64 nested loops, FMA tolerance

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  132 ms |   35.06 ms |   40.3 KB |
| torajs-run       |     —   |   42.69 ms |          — |
| **rust**         |   74 ms | **35.05 ms ← (essentially tied, within 0.03%)** |     455 KB |
| go               |   38 ms |   37.26 ms |    2.26 MB |
| bun-aot          |   57 ms |   51.86 ms |      60 MB |
| bun-jsc          |     —   |   52.22 ms |          — |
| node-v8          |     —   |  125.76 ms |          — |

#### `popcount` — LLVM loop-idiom recognition (BK pattern → ARM `cnt.16b` NEON)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  132 ms |  **2.64 ms ← (1.06× faster than rust)** | **40.3 KB** |
| torajs-run       |     —   |    9.55 ms |          — |
| rust             |   74 ms |    2.80 ms |     455 KB |
| go               |   37 ms |   57.48 ms |    2.26 MB |
| bun-aot          |   57 ms |   58.21 ms |      60 MB |
| bun-jsc          |     —   |   57.01 ms |          — |
| node-v8          |     —   |  137.53 ms |          — |

#### `prime_count` — trial division, bool return, early-return-from-while

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  136 ms |   48.18 ms |   40.3 KB |
| torajs-run       |     —   |   55.27 ms |          — |
| rust             |   76 ms |   48.52 ms |     455 KB |
| **go**           |   38 ms | **40.37 ms ←** |    2.26 MB |
| bun-aot          |   58 ms |   51.91 ms |      60 MB |
| bun-jsc          |     —   |   52.43 ms |          — |
| node-v8          |     —   |  155.23 ms |          — |

#### `startup` — program launch cost (cold-start perf)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  130 ms |  **1.25 ms ← (cold-start champion)** | **40.2 KB** |
| torajs-run       |     —   |    8.04 ms |          — |
| rust             |   69 ms |    1.35 ms |     455 KB |
| go               |   39 ms |    1.91 ms |    2.26 MB |
| bun-aot          |   57 ms |    7.82 ms |      60 MB |
| bun-jsc          |     —   |    9.02 ms |          — |
| node-v8          |     —   |   84.10 ms |          — |

#### `throw-catch-100k` — 100k handled exceptions through 2 stack frames

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  132 ms |  **1.55 ms ← (277× faster than rust panic)** | **40.3 KB** |
| torajs-run       |     —   |    9.66 ms |          — |
| rust             |   87 ms |  429.19 ms |     458 KB |
| go               |   39 ms |    7.49 ms |    2.26 MB |
| bun-aot          |   58 ms |   23.59 ms |      60 MB |
| bun-jsc          |     —   |   23.75 ms |          — |
| node-v8          |     —   |  147.62 ms |          — |

`throw-catch-100k` exercises tr's M4 throw model at saturation: every `may_throw` call site emits a `cond_br` on `__torajs_throw_active`, so a quiet path costs one untaken branch per call and a thrown path resumes at the nearest catch without unwinding any frame metadata. Rust pays for `panic::catch_unwind` per-occurrence (DWARF unwind info, drop landing pads), which explains the gap.

### Aggregate compile / binary

|       runtime       | median compile | median binary |
| ------------------- | -------------: | ------------: |
| **torajs (AOT)**    |    **~133 ms** |  **40.3 KB**  |
| go                  |          ~38 ms |       2.26 MB |
| bun-aot             |          ~58 ms |        60 MB |
| rust                |          ~76 ms |       455 KB |

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
