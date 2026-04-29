# torajs roadmap

> Canonical implementation plan. Living document — update as work progresses, decisions change, or steps reveal new sub-steps.
>
> Last revised: 2026-04-30 (TS-subset pivot — discarded "Rust-shaped semantics" framing; rewrote milestones around bun-shape feature parity with compile-time ownership inference)
>
> Provenance audit trail: `.claude/researches/0001-direction.md` through `0005-roadmap.md` (early discussion logs — note: pre-2026-04-30 they used a "TS syntax + Rust semantics" framing that was takagi-corrected on 2026-04-30; treat them as historical context, not as design source-of-truth).

---

## Foundation

### Goal

Build a TypeScript runtime that runs a **subset of TS programs** with **TS semantics** — same observable behavior as `bun` running the same source. The differentiator is the runtime: AOT-compiled to a small native binary via LLVM, JIT-executed via Cranelift, with **compile-time ownership inference** instead of GC.

When behavior is ambiguous, **bun is the oracle**. Write the equivalent in TS, run it in `bun`, and match.

### Hard requirements

1. **极致 perf** — beat bun/node on important benchmarks; hold them. Already 8/8 wins vs bun-jsc, 8/8 wins vs node-v8.
2. **Compile not too slow** — rustc-debug-class for dev (single-digit-ms compile via Cranelift); LLVM `--release` opt-in for production builds (~45 ms / case).
3. **Interpretable** — REPL + dev test runner without AOT compile (`tr run` via Cranelift JIT).
4. **No GC** — no tracing GC, no refcount, no runtime memory-management overhead. The compiler infers ownership at compile time and emits deterministic drops.
5. **TS-shape semantics** — what works, works the same as bun. No Rust-flavored idioms in user code (no `.clone()`, no `Rc<T>`, no lifetime annotations).
6. **TS subset** — partial coverage of TS surface. Programs the compiler can't statically resolve (multi-rooted ownership, certain dynamic patterns) get clear compile errors. Users restructure to fit the subset.

### What's NOT in scope (corrections from earlier framing)

Earlier roadmap drafts (pre-2026-04-30) called out **"TS syntax + Rust-shaped semantics"** with explicit `Rc<T>` / affine moves / `.clone()` exposed to the user. takagi corrected this on 2026-04-30: **torajs is a TS subset, not a Rust dialect**. User-visible Rust idioms are out:

- `Rc<T>` / `Arc<T>` / `RefCell<T>` — never user-facing
- `.clone()` as a required call — compiler decides
- Lifetime annotations `'a` — none
- `&` / `&mut` reference operators — none
- `move` keyword — not needed
- Affine "use of moved value" errors on simple read sites — replaced by alias-aware ownership inference

The compiler still does ownership analysis under the hood (the no-GC requirement leaves no choice), but it's **invisible at the source level**. See `docs/ts-subset.md` for the user-facing subset boundary, including the small set of programs that get rejected at compile time (multi-rooted ownership) with restructuring suggestions.

### Resolved decisions

| Decision | Choice |
| --- | --- |
| Engine implementation language | Rust |
| Source language | TS subset (TS surface, TS semantics, partial coverage) |
| Embed existing JS engine? | No — write our own |
| Execution model | AOT (`tr build`) + JIT (`tr run`) — both consume same SSA IR |
| Memory model | Compile-time ownership inference; no GC, no refcount |
| Compiler backend | LLVM via Inkwell (AOT) + Cranelift (JIT) — same SSA IR |
| TS conformance | None — torajs is a subset, not aligned to any TS version |
| Test262 conformance | Not a goal |
| First-class WASM target | Yes — torajs.com playground depends on it |

### Working mode

- Closed-source research project. Many experiments will be discarded.
- New ideas land in `labs/` first; graduate to `crates/` when stable.
- Drive forward — execute the committed milestones below without per-step asking. Stop only for genuine forks (design questions not in this doc, irreversible decisions, ambiguous-recovery failures).
- See `.claude/rules/common/` and `.claude/rules/{rust,typescript}/` for shared coding standards. `labs/` may relax them.

---

## Status snapshot (2026-04-30)

After the TS-subset pivot — revert of P2.3 (`Rc<T>` chain) + alias-aware ownership inference shipped.

### What works end-to-end

```
$ tr build foo.tora.ts -o foo  # AOT — LLVM 22 + Inkwell, ~33 KB binary
$ ./foo                         # native execution, perf-leading on bench
$ tr run foo.tora.ts            # JIT — Cranelift, dev-loop, ~5 ms compile
```

A program of this shape compiles and runs end-to-end:

```ts
type Person = { name: string, age: number };

function greet(p: Person): string {
  return p.name;
}

let alice: Person = { name: "alice", age: 30 };
let bob: Person = alice;
console.log(greet(alice));
console.log(bob.name);
console.log(Math.PI);
console.log(Math.floor(Math.sqrt(alice.age * alice.age)));
//                                                   ↑
// scope ends — one drop fires (the struct + its inner string),
// no .clone() ever written by the user, no GC pause.
```

`alice` and `bob` are TS-style aliases of the same heap. `console.log(alice.name)` and `console.log(bob.name)` both work. The compiler infers that `bob` owns at scope-end (`alice` transferred at the let), drops once, no leak (verified via `leaks --atExit`).

### Phases done

| phase | what | done? |
|---|---|---|
| **P0**     | walking skeleton (`tr run hello.ts`) | ✓ |
| **P1**     | core language (10 sub-steps) | ✓ |
| **P2.1+** | alias-aware ownership inference (no GC, TS-shape shared reads) | ✓ 2026-04-30 |
| **P2.2**   | string runtime + drop emission (concat doesn't consume; TS-shape) | ✓ |
| **P2.4**   | object literals + structural types | ✓ |
| **P3**     | LLVM AOT + Cranelift JIT (one SSA IR, two backends) | ✓ |
| **stdlib slice 1** | `console.log`, `Math.{sqrt,abs,floor,ceil,log,exp,pow,min,max,PI,E}`, `String.length`, `print_f64` | ✓ |
| ~~P2.3~~ | ~~`Rc<T>` first-class~~ | **REMOVED** — incompatible with TS-subset framing |

### Bench position (M4 Pro, hyperfine n=3-10)

8 of 8 cases green on `torajs` (AOT) + `torajs-jit`. Vs **rust**: 5 wins / 3 ties / 0 losses on AOT. Vs **bun**: 8/8 wins on every case. Vs **node-v8**: 8/8 wins (4-71×). See `README.md` for the full table.

### Code size

```
labs/0001-walking-skeleton/src/   ~5500 LOC across 9 files
docs/                              roadmap.md + stdlib.md + ts-subset.md
bench/                             8 cases × 5 langs + harness + runners + results
```

### Currently executing

The TS-subset pivot just landed (commits `d391a8d` revert + `26f74e2` alias-aware + `cbaa360` subset docs). Next up: **M1.1 — line/block comments in lexer**, then chain through M1 (TS subset core completeness).

---

## Execution path — committed order, no per-step ask

The single committed path through the TS-subset implementation. Each milestone is a coherent chunk of TS-surface coverage; sub-steps within a milestone roll up to one observable feature shipping. The agent runs each step end-to-end (code + tests + commit) without checkpointing back unless a real fork appears.

### M1 — finish the TS subset core

Plug remaining holes in the language surface so non-trivial TS programs run.

| step | what | exit gate |
|---|---|---|
| **M1.1** | line + block comments (`//`, `/* */`) in lexer | bench cases with comments parse + run |
| **M1.2** | `Array<T>` runtime — alloc, `push`, `length`, indexing, drop | `let xs: number[] = []; xs.push(1); xs.push(2); console.log(xs.length);` works end-to-end on JIT + AOT |
| **M1.3** | block-scope drops (currently only fn-level — `if (cond) { let x = ...; }` leaks until fn end) | inner-block heap drops at `}` boundary; verified via `leaks --atExit` |
| **M1.4** | mutable struct field write `p.x = v` (currently rejected at Assign target check) | enable Member targets in Assign; emit drop for old field value if non-Copy |
| **M1.5** | boolean ops `&&`, `\|\|`, `!` with TS truthy/falsy semantics | `if (a && b) { ... }` works |
| **M1.6** | `for (let i = 0; i < n; i++)` C-style for-loop | bench cases use this idiomatically; currently rewritten to `while` |
| **M1.7** | `break` / `continue` | currently no loop control |

Exit gate for M1: a non-trivial TS program (linked-list traversal, fibonacci with memoization via array, simple state machine) runs in torajs end-to-end on JIT + AOT, no leaks.

### M2 — Closures with implicit captures

| step | what | exit gate |
|---|---|---|
| **M2.1** | capture analysis: classify each free var as Move / Borrow / BorrowMut at compile time | analysis output dumpable via `tr capture <file>` |
| **M2.2** | closure environment lowering: heap-allocated env struct, fn ptr + env pair | non-capturing closures unchanged; capturing closure runs end-to-end |
| **M2.3** | shared captures across two closures (compiler resolves via alias inference) | shared-counter pattern works |
| **M2.4** | bench cases: `closure-sum`, `closure-counter`, `closure-iter-fold` | 3 new bench rows green |

### M3 — Generics in user code

The compiler already has `Array<T>` natively after M1. M3 generalizes the mechanism so users can write generic functions and types.

| step | what | exit gate |
|---|---|---|
| **M3.1** | parser/AST: type params on fn decls, type aliases, struct types | `function id<T>(x: T): T { return x; }` parses + typechecks |
| **M3.2** | monomorphization at SSA boundary | `id<number>(5)` and `id<string>("x")` lower to distinct concrete fns |
| **M3.3** | generic structs: `type Pair<A, B> = { fst: A, snd: B }` | round-trips |

### M4 — Error model: try / catch / throw

| step | what | exit gate |
|---|---|---|
| **M4.1** | parser/AST/typecheck for `throw`, `try`, `catch` | typechecks; basic `Error` class |
| **M4.2** | unwinding lowered as state machine + early-return through scope drops | thrown errors propagate through fn boundaries with correct drop ordering |
| **M4.3** | stdlib `Error` / `TypeError` / `RangeError` | matches bun's Error hierarchy (subset) |

### M5 — Module system

| step | what | exit gate |
|---|---|---|
| **M5.1** | `import { x } from "./y.ts"` parser + AST | parses |
| **M5.2** | path resolution from `tora.toml` project root | cross-file references resolve |
| **M5.3** | per-module typecheck + cross-module type unification | type defined in `a.ts`, used in `b.ts`, structurally equal |
| **M5.4** | incremental compilation: cache per-module SSA | second `tr build` of unchanged code finishes < 50 ms |
| **M5.5** | stdlib reorganization: `Math.*` etc. become `import { ... } from "@torajs/std"` | bench unchanged |

**Graduation point**: end of M5, `labs/0001-walking-skeleton/` promotes to `crates/torajs-core/` + `crates/tr-cli/`. `v0.1` tag.

### M6 — Standard library expansion (bun-shape subset)

| step | what | exit gate |
|---|---|---|
| **M6.1** | `String` methods: `slice`, `substring`, `indexOf`, `split`, `join` | full method set tested vs bun |
| **M6.2** | `Array` methods: `map`, `filter`, `reduce`, `forEach`, `find`, `slice` (gates on M2 closures) | full method set tested vs bun |
| **M6.3** | `Date`, `JSON.parse` / `JSON.stringify` | round-trip via tests |
| **M6.4** | `fs` (sync subset): `readFileSync`, `writeFileSync` | reads/writes a file end-to-end |
| **M6.5** | `Bun.file`, `Bun.write` (bun-namespace subset) | matches bun's surface |

### M7 — Async / await

| step | what | exit gate |
|---|---|---|
| **M7.1** | parser/AST/typecheck for `async fn` and `await` | async fn signatures typecheck; await on non-Future errors |
| **M7.2** | state machine lowering: each `await` yields control to the executor | runtime can poll a state machine to completion |
| **M7.3** | single-threaded executor (Tokio-shape, no thread pool) | `tr run` of an async program executes to completion |
| **M7.4** | `Promise.all` / `Promise.race` / `Promise.allSettled` | combinator bench passes |
| **M7.5** | async closures (gates on M2) | works |
| **M7.6** | `fetch` (HTTP via reqwest) | round-trip a real GET request |

Multi-threaded executor + `Send` / `Sync` deferred to M9 (post-v1.0); single-threaded async covers the bun-parity surface for v1.0.

### M8 — Playground + tooling

| step | what | exit gate |
|---|---|---|
| **M8.1** | wasm32 target: engine compiles to wasm, runs in browser | `tr run "console.log('hi')"` in a worker page |
| **M8.2** | torajs.com/playground: editor + run button + share-link | live |
| **M8.3** | bench scoreboard auto-rendered from `bench/results/` | auto-update on commit |
| **M8.4** | LSP server: hover / goto-def / diagnostics | VS Code extension shows hovers + jumps |
| **M8.5** | formatter, linter, `tr test` runner | `tr fmt`, `tr lint`, `tr test` work |

### M9 — Polish + integration → v1.0

| step | what | exit gate |
|---|---|---|
| **M9.1** | DWARF debug info on AOT; source-mapped panic backtraces | crash backtrace points at `.tora.ts` line |
| **M9.2** | `tr debug` step-debugger via Cranelift JIT | step into / step over works |
| **M9.3** | `tr repl` interactive loop | `tr repl` evaluates expressions live |
| **M9.4** | `libtora.a` + `tora_eval()` for embedding in Rust hosts | embed in a Rust app, run a script |
| **M9.5** | multi-threaded executor + `Send`/`Sync` (post-v1.0 stretch) | parallel mandelbrot scales linearly |

`v1.0` tag at end of M9.

Total time-to-v1.0 estimate: 12–24 months, depending on stdlib scope creep at M6.

---

## Principles

- **Every step is visible** — at the end of each step, there's a command you can run and see output. No "internal-only" steps.
- **Small grain** — each step is roughly 1-3 days of work, ~100-500 LOC. If a step grows past that, it splits.
- **Front-loaded detail** — milestones close to now are spelled out per-step; far milestones are headers + exit gates. We re-detail later milestones when we get there.
- **Each step is potentially throwaway** — research mode. If a step's outcome surprises us, we revisit before continuing.
- **bun is the oracle** — when behavior is ambiguous, write the TS equivalent, run in `bun`, match.
- **All P0-P3 work lives in `labs/0001-walking-skeleton/`**. Graduation to `crates/` happens at end of M5 (modules) when the API surface stabilizes.

---

## Backend pivot (2026-04-28) — historical

Through P3.1–P3.3 the AOT path was **wasm-via-C**: tr → wasm-encoder → wasm2c (wabt) → clang -O3 → native binary. This won the bench but had hard ceilings — `compile_ms` floor ~95 ms, no GC integration, no tail calls, no exceptions, external dep on wabt + Apple clang. Replaced with **two backends sharing one SSA IR**:

```
frontend (lex → parse → check) → SSA IR (rich types, ownership-aware,
                                          partial-evaluated, pattern-matched)
                                  ↓                ↓
                         Inkwell (LLVM 22)    Cranelift
                              ↓                  ↓
                         AOT object         JIT in-memory
                         + system ld         + execute
                              ↓                  ↓
                         `tr build`         `tr run`
                         (run_ms 极致)       (compile_ms 极致)
```

Both modes are first-class. `tr build` is the perf-leading native binary. `tr run` is Go-style "compile to memory and execute".

### run_ms ceiling = three layers, not one

```
run_ms 极限 = optimal_codegen × optimal_runtime × optimal_layout
            = LLVM            × no-GC ownership × Rust-style layout
```

Picking LLVM solved the codegen layer. The runtime layer (no GC, deterministic drop) is where bun/V8 lose 4–20× — their codegen is fine, their runtime + layout carry too much overhead. Specific commitments:

1. **IR carries rich type info** → emit specialized LLVM IR. Monomorphization, devirtualization at IR level, `noalias` from ownership analysis, `!range` from type narrowing.
2. **Compile-time ownership inference** — alias-aware analysis under TS-shape semantics. Deterministic drops at scope exit, no GC pause, no refcount bumps. (Pre-pivot framing called for `Rc<T>` as user-visible escape valve; that was wrong — corrected 2026-04-30.)
3. **Language-level PGO** — `@hot` / `@cold` attributes → LLVM `branch_weights` metadata.
4. **Pattern-detected intrinsics** — Brian Kernighan popcount → `@llvm.ctpop.i64`, ctz/clz/bswap, vectorizable nested loops → NEON.
5. **Stack/arena allocation first** — escape analysis to stack-allocate non-escaping locals; region inference for fn-scoped temporaries.
6. **Apple Silicon tuning** — Apple LLVM beats upstream LLVM by ~7% on M-series; Inkwell links against `/usr/lib/libLLVM.dylib` on darwin.
7. **Compile-time partial evaluation** — const folding, template literal concat, `[1,2,3].length → 3` happen in IR before LLVM.

---

## BENCH — cross-runtime perf benchmark (cross-cutting track)

A horizontal track running alongside every milestone, not numbered as a phase. Lives at `bench/` (top-level), implemented as a Rust harness crate that drives **bun, node, rust, go, python**, and torajs through a uniform per-case workload.

### Status (2026-04-30)

8 cases × 5 runtimes + torajs (AOT + JIT). All 8 cases green on both torajs paths:

```
case          torajs (AOT)  torajs-jit  rust    go     bun-jsc  node-v8
ackermann          8.58       19.62     8.75    9.62   15.06     97.67
collatz          106.10      210.73   105.57  142.60  322.04   1399.15
fib40            146.82      515.58   178.56  227.26  382.48    641.08
gcd1m             40.23       50.06    40.71   38.78   46.06    127.78
mandelbrot        34.92       89.28    33.61   35.40   49.20    121.45
popcount           2.91      105.13     2.72   55.33   55.29    127.66
prime_count       47.94       55.45    47.67   39.06   58.72    159.45
startup            1.14        7.60     1.34    1.82    7.86     83.14
```

### Adding a case

Drop a directory under `bench/cases/<name>/` with `main.<lang>` files, an `expected.txt`, and an optional `bench.toml` (runs / warmup / `torajs_opt` knob). The harness skips runners whose source file is missing — so a case can be torajs-only or torajs-+-rust if the workload doesn't translate to other langs.

**Rule (per `feedback_bench_tr_must_pass.md`)**: every committed bench case must have torajs producing `ok`. A case where torajs appears as `fail` (because the language doesn't support that workload yet) is treated as the milestone not having been achieved. The bench scoreboard and torajs's language capability grow in lockstep.

---

## Cross-cutting tracks

Work that runs **alongside** every milestone, not as one of them. Tracked here so it stays visible.

### Test infrastructure

- **Per-milestone acceptance criteria** — each row above carries its own exit gate. Cumulative test count drives a regression net.
- **Bench scoreboard as integration test** — every case is an end-to-end test; a regression there is a P0.
- **Integration test crate** at `crates/torajs-itest/` (post-graduation) runs full `tr build` + execute on every example under `examples/`. CI gate.
- **Property testing** — quickcheck-style for the type checker's alias-aware ownership analysis (random ASTs, must accept TS-valid programs and reject multi-rooted ones). Lands when alias bugs surface.
- **Fuzzing** — `cargo fuzz` targets for the lexer + parser. Lands during the `labs/` → `crates/` graduation.

### CI / release process

- **GitHub Actions on `develop`**: per-commit `cargo build` + `cargo test` + `cargo clippy --workspace --all-targets -- -D warnings` + `bun run check` for `web/`. Gates merge.
- **Release branches** per `git-flow`. `main` is production; `develop` is integration; milestones close on `develop` and roll up to `main` at tags (`v0.1` after M5, `v0.5` after M8, `v1.0` after M9).
- **Tag-driven artifact publishing** on tag: build `tr` binary for darwin-aarch64 + linux-x86_64 + linux-aarch64 + windows-x86_64, package as a tarball, attach to GH release. Distributed via a future `tora-up` install script.

### Documentation

- **`docs/` is canonical** — this roadmap, `stdlib.md`, `ts-subset.md`, future `lang-reference.md`, `embedding.md`. Versioned with the code.
- **Public website** at `torajs.com` — landing + playground (M8) + docs + bench scoreboard (auto-generated from `bench/results/`).
- **No external blog/marketing** during research phase. Communications happen on takagi's discretion.

### Performance work as a continuous track

Perf work happens incrementally:
- After P3 closeout: codegen baseline established.
- During M1-M5: avoid regressing existing bench cases as features land.
- After M5 (graduation): formal perf RFCs land — bit-packing for bool, SoA layouts for hot loops.
- After M3 (generics): monomorphization-driven inlining tweaks.
- After M9: source maps unlock profiler workflows — perf work becomes profile-guided.

### Security / threat model (for embedding + playground)

- **CLI binary** — runs trusted user code; same threat model as Node/Bun.
- **Embedding API** (M9) — runs partially-trusted scripts. Sandboxing knobs mandatory; off-by-default = unsafe.
- **Playground** (M8) — runs untrusted code in an isolated wasm worker, hard memory + CPU caps. Fresh instance per Run.
- **No supply-chain story** until package manager exists. Stdlib + user-relative imports only — no third-party packages can introduce vulnerabilities.

---

## Out-of-scope features

Things explicitly NOT planned. Some have been demoted from earlier drafts (under the wrong "Rust semantics" framing):

- **`null` / `undefined`** — dropped by design. Use `Option<T>` if needed (TS subset doesn't have undefined either; explicit absence via tagged unions).
- **`==` / `!=`** — only `===` / `!==`.
- **`var` keyword** — only `let` / `const`.
- **Decorators** — not planned. Use cases are better served by macros (far) or manual code.
- **JSX** — out of scope.
- **`Symbol` / `Proxy` / `Reflect` / `WeakMap` / `WeakRef`** — dropped. Static typing makes most unnecessary; no-GC runtime makes the rest unsound.
- **`eval` / `Function` constructor** — dropped.
- **Class syntax (initial)** — possibly later as desugaring to `type` + `impl`-style methods. Not in M1-M9 critical path.
- **Conditional / mapped types** (`Pick<T, K>`, `Partial<T>`, `T extends U ? X : Y`) — TS-specific compiler tricks bound to its inference model. Probably never.
- **Cycle-collecting weak references** — no-GC contract. Cycles in dynamic structures (mutable graph nodes referencing each other) are not expressible in the TS subset; users restructure. No `Weak<T>` shipping.
- **`Rc<T>` / `Arc<T>` / `RefCell<T>` user-visible types** — corrected on 2026-04-30. The runtime implementation may use refcount-like techniques internally, but these are NEVER user-facing.
- **Test262 conformance** — out of scope.
- **WebAssembly user-code target** (different from "engine-as-wasm" in M8) — emit wasm artifacts from user `.tora.ts` source for non-browser deployment. Beyond v1.0 timeline.
- **Multi-threaded executor + `Send`/`Sync`** — single-threaded async is enough for v1.0 (matches bun's main path). Multi-threaded deferred to M9.5 (post-v1.0).

---

## Historical phase numbering (P0–P17)

Pre-2026-04-30 the roadmap used phase-numbered sections (P0, P1, P2, ..., P17) with detailed sub-step breakdowns. Those sections were written under the now-discarded "TS syntax + Rust semantics" framing. They contained accurate descriptions of P0/P1/P2.4/P3/stdlib slice 1 (which all shipped under TS-shape semantics regardless of framing), but the P2/P4-P17 plans baked in Rust-specific concepts (`Rc<T>`, affine moves, `Send`/`Sync` ownership types, `'a` lifetimes) that were corrected.

The committed forward-looking plan is **M1-M9 above**. The P0-P17 sections have been removed from this document; for archival reference of what was discussed pre-pivot, see git history (commit `4892919` for the P3-onward industrial plan; commit `84241d9` for the M1-M9 pre-pivot version).
