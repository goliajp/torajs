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

Cross-runtime perf, M4 Pro, hyperfine n=10 with 3 warmup runs. Measured 2026-05-02 on commit `7e39849` (load avg ~5). Latest additions vs `d54e95e`: `instanceof` operator (compile-time class membership — direct identity + extends-chain walk via `Ast.class_parents` recorded by desugar_classes); variadic `String.fromCharCode(...codes)` chain; generic class fixed-point monomorphization (`class Wrapper<T>`, `class Stack<T>`, `class Pair<A, B>`) with deep-cloned mono bodies + multi-type-param defaults via `__tvdefault__T` markers. Earlier in this run: object spread, Math.random, Number.toString(radix), JSON.stringify (recursive type-aware serializer), multi-arg console.{log,error,warn}, default function parameters, rest parameters, spread in fn args, Array.isArray (compile-time), Map class pattern, integration ports across try/throw/switch/destructuring/template+spread/null+optchain/array pipeline/class-deep/defparam-combo/stdlib-grid/deep-recursion/string-recursion/JSON-roundtrip, plus the prior batch (Math trig/hyperbolic, String.{trim*,padStart,padEnd,replace,replaceAll,at,localeCompare}, Array.{includes,findIndex,some,every,reverse,fill,sort,flat,concat,copyWithin,lastIndexOf,at}, Number.{parseInt,parseFloat,isInteger,isNaN,isFinite,isSafeInteger,toFixed,toExponential,toPrecision,toString}, Math.{imul,clz32,fround} + constants, Number(x) / String(x) coercion, console.error/warn → stderr, bare-name globals, lexer escapes + scientific notation, post `++`/`--` spec, struct-field push, class array-field default, return-via-let closure detection, closure-captured array push env writeback, empty `[]` inner literals, ssa-lower auto-coerce return value i64→f64). All times in ms; binary in KB / MB. [Full JSON data](bench/results/).

> **Noise note.** Short-time cases (< 20 ms — startup, generic-id, generic-pair, popcount, throw-catch, closure-pipeline) carry ±15% scheduling noise on a shared M4 Pro at load avg > 3. The 15-run history under `bench/results/` shows e.g. `tr generic-id-1m` ranging 12.26 ~ 18.45 ms; the table below reflects this run, not a hand-picked best. Sanity check: rust source is unchanged across the last 5 bench runs but rust's own numbers move ±10% on the same cases — confirming any tr movement at that scale is machine, not code. Long cases (collatz, fib40, gcd1m, mandelbrot, prime_count, closure-counter) stay within ±2% across the entire history.

### Headline summary (run-time, lower better)

|     case                  | torajs (AOT)  |   torajs-run  |       rust |         go |    bun-jsc |    bun-aot |    node-v8 |
| ------------------------- | ------------: | ------------: | ---------: | ---------: | ---------: | ---------: | ---------: |
| **ackermann**             |      **9.10** |         16.17 |       9.14 |      10.77 |      16.92 |      17.02 |     103.69 |
| array-map-1m              |         29.18 |         38.63 |      28.45 |  **20.16** |      60.24 |      61.51 |     244.87 |
| **array-sum-1m**          |     **11.53** |         19.53 |      13.82 |      31.50 |      46.07 |      50.78 |     180.92 |
| **closure-counter**       |     **18.20** |         25.61 |      18.48 |      36.89 |      46.86 |      47.61 |     179.11 |
| **closure-pipeline-1m**   |     **16.68** |         20.65 |      19.24 |      35.53 |      49.81 |      47.65 |     179.05 |
| collatz                   |        106.41 |        114.28 | **106.13** |     143.43 |     327.49 |     328.14 |    1411.11 |
| **fib40**                 |    **147.25** |        239.01 |     181.77 |     232.51 |     383.58 |     380.58 |     697.62 |
| **gcd1m**                 |     **40.48** |         48.05 |      41.01 |      41.32 |      49.75 |      50.33 |     129.31 |
| **generic-id-1m**         |     **13.79** |         26.46 |      13.81 |      33.76 |      46.70 |      48.37 |     176.27 |
| **generic-pair-1m**       |      **1.42** |          8.19 |       2.15 |       2.92 |      13.65 |      13.61 |      86.92 |
| mandelbrot                |         35.35 |         44.09 |  **35.46** |      37.65 |      52.88 |      52.99 |     125.17 |
| **popcount**              |      **2.64** |          9.98 |       2.73 |      52.25 |      58.09 |      58.67 |     133.73 |
| **prime_count**           |     **47.79** |         55.22 |      48.53 |      40.27 |      58.65 |      53.39 |     160.89 |
| **startup**               |      **1.41** |          8.36 |       1.49 |       1.89 |       9.48 |       8.25 |      84.34 |
| **throw-catch-100k**      |      **1.29** |          8.21 |     426.23 |       7.32 |      23.80 |      23.60 |     147.57 |

torajs (AOT) **vs rust**: **7 wins / 7 ties (within 2%) / 1 loss** (array-map-1m, where go's append fast path also outpaces rust). Of note vs the prior run: `generic-id-1m` is now tied 13.79 vs 13.81, `closure-pipeline-1m` slid back from a previous-run best 11.39 to a more typical 16.68 (still 1.15× faster than rust), and `throw-catch-100k` tightened from 1.55 → 1.29 ms.
torajs (AOT) **vs go**: 13 wins, 2 losses (array-map-1m and prime_count — go's per-element fast path + GC-backed tight loops).
torajs (AOT) **vs bun-jsc / bun-aot / node-v8**: **15 / 15 / 15** clean sweeps per runtime.

`throw-catch-100k` stays the category-killer: 100k handled exceptions takes 1.29 ms in torajs vs 426 ms in rust — **330× faster than rust's panic path**. tr's M4 design (module-level throw_active flag + cond_br on every may_throw call) lets throw be ~zero-cost when it doesn't fire.

65+ commits since `7c7844e` added zero perf regression on the scoreboard's stable cases — long cases (fib40, mandelbrot, gcd1m, collatz, prime_count, closure-counter) all sit within ±2% of their `7c7844e`-era values across every bench since. Conformance grew from 197 → **259 ports** — Math 100% complete, object spread, default + rest params + spread call args, JSON.stringify, multi-arg console, variadic String.fromCharCode + fromCodePoint, codePointAt, Number.toString(radix), generic class fixed-point monomorphization (`class Wrapper<T>`, `class Stack<T>`, `class Pair<A, B>`), `instanceof` (direct + extends-chain walk), `Array.from(string)`, and many integration ports. See [docs/100-percent-plan.md](docs/100-percent-plan.md) for the subset-expansion roadmap toward 100% test262 coverage.

### Per-case detail — compile / run / binary

Each case shows: AOT compile time, AOT execution time, dev-loop `tr run` execution time, and on-disk binary size. `—` marks runtimes that don't have that mode (`tr run` is AOT-with-cache, no separate binary; bun-jsc/node-v8 have no detached compile step).

#### `ackermann` — nested recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  132 ms |  **9.10 ms ←** | **40.3 KB** |
| torajs-run       |     —   |   16.17 ms |          — |
| rust             |   75 ms |    9.14 ms |     455 KB |
| go               |   39 ms |   10.77 ms |    2.26 MB |
| bun-aot          |   63 ms |   17.02 ms |      60 MB |
| bun-jsc          |     —   |   16.92 ms |          — |
| node-v8          |     —   |  103.69 ms |          — |

#### `array-map-1m` — 1M-element `Array<number>::map` over a closure

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  134 ms |   29.18 ms |   40.4 KB |
| torajs-run       |     —   |   38.63 ms |          — |
| rust             |   97 ms |   28.45 ms |     455 KB |
| **go**           |   39 ms | **20.16 ms ← (per-element fast path)** |    2.26 MB |
| bun-aot          |   68 ms |   61.51 ms |      60 MB |
| bun-jsc          |     —   |   60.24 ms |          — |
| node-v8          |     —   |  244.87 ms |          — |

The current weak spot — go's `append` + bounds-check elision and rust's `Vec::push` cap-doubling outpace tr's amortized-realloc on bulk-grow. tr still beats every JS runtime by 2×+ on this case.

#### `array-sum-1m` — 1M `Array<T>::push` + index sum (heap alloc + amortized realloc)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  133 ms | **11.53 ms ← (1.20× faster than rust)** | **40.3 KB** |
| torajs-run       |     —   |   19.53 ms |          — |
| rust             |   83 ms |   13.82 ms |     455 KB |
| go               |   38 ms |   31.50 ms |    2.26 MB |
| bun-aot          |   67 ms |   50.78 ms |      60 MB |
| bun-jsc          |     —   |   46.07 ms |          — |
| node-v8          |     —   |  180.92 ms |          — |

#### `closure-counter` — long-lived closure mutating captured state across calls

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  134 ms | **18.20 ms ← (within 1.5%)** | **40.4 KB** |
| torajs-run       |     —   |   25.61 ms |          — |
| rust             |   91 ms |   18.48 ms |     455 KB |
| go               |   39 ms |   36.89 ms |    2.26 MB |
| bun-aot          |   60 ms |   47.61 ms |      60 MB |
| bun-jsc          |     —   |   46.86 ms |          — |
| node-v8          |     —   |  179.11 ms |          — |

#### `closure-pipeline-1m` — 10M indirect calls through fn-pointer arg

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  134 ms | **16.68 ms ← (1.15× faster than rust)** | **40.4 KB** |
| torajs-run       |     —   |   20.65 ms |          — |
| rust             |   85 ms |   19.24 ms |     455 KB |
| go               |   38 ms |   35.53 ms |    2.26 MB |
| bun-aot          |   59 ms |   47.65 ms |      60 MB |
| bun-jsc          |     —   |   49.81 ms |          — |
| node-v8          |     —   |  179.05 ms |          — |

Rust uses `black_box(add1 as fn(i64)->i64)` to defeat fn-pointer devirtualization; torajs always emits a real `CallIndirect`. Apples-to-apples indirect call.

#### `collatz` — bit ops + hailstone trajectory + outer/inner loop

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  133 ms |  106.41 ms |   40.3 KB |
| torajs-run       |     —   |  114.28 ms |          — |
| **rust**         |   75 ms | **106.13 ms ← (essentially tied, within 0.3%)** |     455 KB |
| go               |   39 ms |  143.43 ms |    2.26 MB |
| bun-aot          |   59 ms |  328.14 ms |      60 MB |
| bun-jsc          |     —   |  327.49 ms |          — |
| node-v8          |     —   | 1411.11 ms |          — |

#### `fib40` — recursion, integer arithmetic

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  129 ms | **147.25 ms ← (1.23× faster than rust)** | **40.2 KB** |
| torajs-run       |     —   |  239.01 ms |          — |
| rust             |   74 ms |  181.77 ms |     455 KB |
| go               |   39 ms |  232.51 ms |    2.26 MB |
| bun-aot          |   59 ms |  380.58 ms |      60 MB |
| bun-jsc          |     —   |  383.58 ms |          — |
| node-v8          |     —   |  697.62 ms |          — |

#### `gcd1m` — tight integer loop with mod

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  132 ms | **40.48 ms ← (1.3% faster than rust)** | **40.3 KB** |
| torajs-run       |     —   |   48.05 ms |          — |
| rust             |   75 ms |   41.01 ms |     455 KB |
| go               |   38 ms |   41.32 ms |    2.26 MB |
| bun-aot          |   60 ms |   50.33 ms |      60 MB |
| bun-jsc          |     —   |   49.75 ms |          — |
| node-v8          |     —   |  129.31 ms |          — |

#### `generic-id-1m` — 1M calls through a monomorphized generic identity fn

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  146 ms | **13.79 ms ← (essentially tied, within 0.2%)** | **40.4 KB** |
| torajs-run       |     —   |   26.46 ms |          — |
| rust             |   89 ms |   13.81 ms |     455 KB |
| go               |   39 ms |   33.76 ms |    2.26 MB |
| bun-aot          |   59 ms |   48.37 ms |      60 MB |
| bun-jsc          |     —   |   46.70 ms |          — |
| node-v8          |     —   |  176.27 ms |          — |

This is the noisiest case in the suite: tr's 15-run history under `bench/results/` ranges 12.26 ~ 18.45 ms (≈ 50% spread), rust's 11.87 ~ 15.19 (≈ 28% spread). The body is one indirect call per iteration of a tiny loop; both runtimes do the right thing (LLVM monomorphizes, function-call overhead dominates), and the gap on any single run is dominated by OS scheduler noise on the M4 Pro.

#### `generic-pair-1m` — 1M generic struct allocations + field reads

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  144 ms |  **1.42 ms ← (1.51× faster than rust, 9.6× faster than bun)** | **40.3 KB** |
| torajs-run       |     —   |    8.19 ms |          — |
| rust             |   75 ms |    2.15 ms |     455 KB |
| go               |   42 ms |    2.92 ms |    2.26 MB |
| bun-aot          |   72 ms |   13.61 ms |      60 MB |
| bun-jsc          |     —   |   13.65 ms |          — |
| node-v8          |     —   |   86.92 ms |          — |

Monomorphization at codegen flattens `Pair<A, B>` into a stack-shape struct with no boxing; LLVM proceeds to elide most allocations.

#### `mandelbrot` — f64 nested loops, FMA tolerance

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  140 ms |   35.35 ms |   40.3 KB |
| torajs-run       |     —   |   44.09 ms |          — |
| **rust**         |   80 ms | **35.46 ms ← (tora 0.3% faster)** |     455 KB |
| go               |   40 ms |   37.65 ms |    2.26 MB |
| bun-aot          |   60 ms |   52.99 ms |      60 MB |
| bun-jsc          |     —   |   52.88 ms |          — |
| node-v8          |     —   |  125.17 ms |          — |

#### `popcount` — LLVM loop-idiom recognition (BK pattern → ARM `cnt.16b` NEON)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  134 ms |  **2.64 ms ← (1.03× faster than rust)** | **40.3 KB** |
| torajs-run       |     —   |    9.98 ms |          — |
| rust             |   78 ms |    2.73 ms |     455 KB |
| go               |   39 ms |   52.25 ms |    2.26 MB |
| bun-aot          |   62 ms |   58.67 ms |      60 MB |
| bun-jsc          |     —   |   58.09 ms |          — |
| node-v8          |     —   |  133.73 ms |          — |

#### `prime_count` — trial division, bool return, early-return-from-while

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| torajs (AOT)     |  132 ms |   47.79 ms |   40.3 KB |
| torajs-run       |     —   |   55.22 ms |          — |
| rust             |   76 ms |   48.53 ms |     455 KB |
| **go**           |   37 ms | **40.27 ms ←** |    2.26 MB |
| bun-aot          |   60 ms |   53.39 ms |      60 MB |
| bun-jsc          |     —   |   58.65 ms |          — |
| node-v8          |     —   |  160.89 ms |          — |

#### `startup` — program launch cost (cold-start perf)

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  130 ms |  **1.41 ms ← (cold-start champion)** | **40.2 KB** |
| torajs-run       |     —   |    8.36 ms |          — |
| rust             |   69 ms |    1.49 ms |     455 KB |
| go               |   39 ms |    1.89 ms |    2.26 MB |
| bun-aot          |   60 ms |    8.25 ms |      60 MB |
| bun-jsc          |     —   |    9.48 ms |          — |
| node-v8          |     —   |   84.34 ms |          — |

#### `throw-catch-100k` — 100k handled exceptions through 2 stack frames

|     runtime      | compile |        run |     binary |
| ---------------- | ------: | ---------: | ---------: |
| **torajs (AOT)** |  131 ms |  **1.29 ms ← (330× faster than rust panic)** | **40.3 KB** |
| torajs-run       |     —   |    8.21 ms |          — |
| rust             |   88 ms |  426.23 ms |     458 KB |
| go               |   39 ms |    7.32 ms |    2.26 MB |
| bun-aot          |   59 ms |   23.60 ms |      60 MB |
| bun-jsc          |     —   |   23.80 ms |          — |
| node-v8          |     —   |  147.57 ms |          — |

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
