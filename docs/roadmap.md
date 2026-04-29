# torajs roadmap

> Canonical implementation plan. Living document — update as work progresses, decisions change, or steps reveal new sub-steps.
>
> Provenance: synthesized from `.claude/researches/0001-direction.md` through `0005-roadmap.md` (research / discussion logs, kept for audit trail).
>
> Last revised: 2026-04-30 (committed M1→M9 execution path — agent drives the path without per-step ask)

---

## Foundation

### Goal

Build torajs from scratch in Rust: a statically-typed, AOT-compiled language with **TypeScript syntax** as the visual surface and **Rust-shaped semantics** underneath (no GC, ownership, deterministic destruction, async/await, Send/Sync). Closed-source research project. Performance is the structural moat — not "match Bun", but be **categorically faster** on typed code by AOT-compiling with type-directed lowering, since JS engines can't.

### Hard requirements (takagi, 2026-04-26)

1. **极致 perf** — beat Bun/Node/etc on important benchmarks; hold them
2. **Compile not too slow** — rustc-debug-class for dev (5-30s / 10kLOC); LLVM `--release` opt-in for production
3. **Interpretable** — REPL + dev test runner without AOT compile
4. **No GC** — no tracing GC; refcount (`Rc<T>` / `Arc<T>`) allowed as fallback for shared ownership
5. **Multi-core** — Send/Sync in the type system; multi-threaded executor
6. **async/await** — first-class; lower to state machines; pluggable into the executor
7. **TS-shaped syntax** — TS is the visual identity. Compatibility with `tsc`/Bun is a *downstream* engineering concern, not a design driver. We pick the TS surface we want and drop the rest (`enum`/`namespace`/`with`/`var`/`==`/`null`/sloppy mode/decorators/`eval` are out).

### Resolved decisions

| Decision | Choice | Source |
| --- | --- | --- |
| Engine implementation language | Rust | 0001 |
| Source language | TS-shaped (our dialect, not faithful TS) | 0001, 0004 |
| Embed existing JS engine? | No — write our own | 0001 |
| Execution model | AOT (production) + interpreter (dev) — both consume same IR | 0003, 0004 |
| Memory model | Static ownership (affine + explicit `Rc<T>`); no tracing GC | 0003, 0004 |
| Compiler backend (revised 2026-04-28) | **LLVM via Inkwell (AOT) + Cranelift (JIT)**, both consume the same SSA IR. Wasm-via-C path retired — see "Backend pivot (2026-04-28)" below. | session 2026-04-28 |
| Concurrency | Built-in multi-threaded work-stealing executor in std; `Send`/`Sync` traits enforced statically | 0004 |
| Compat with `tsc`/Bun | Not a design driver. Compat layer is downstream future work. | 0004 |
| TS spec / version conformance | None — we pick our own TS-shaped dialect; not aligned to TS6 or any TS version. Re-confirmed 2026-04-26 against a "follow TS6 rules" alternative. | session 2026-04-26 |
| Test262 conformance | Not a goal | 0001 |
| First-class WASM target | Yes — torajs.com playground depends on it | 0001 |
| Project repository home | `crates/` (Rust workspace), `web/`, `labs/`, `examples/`, `docs/` | 0001 |

### Backend pivot (2026-04-28)

Through P3.1–P3.3 the AOT path was **wasm-via-C**: tr → wasm-encoder → wasm2c (wabt) → clang -O3 → native binary. This shipped fast and won the bench against bun/node on every case (and beat rust on fib40 + popcount + startup). But it has hard ceilings that bite at P4:

- `compile_ms` floor ~95 ms — 70 ms of that is clang frontend re-parsing wasm2c's verbose C
- wasm has no GC integration in our toolchain (wasm2c precedes the wasm-GC proposal)
- no tail calls, no exceptions, no SIMD intrinsics in the wabt output
- external dep on Apple clang + homebrew wabt — bad for distribution
- **wasm-via-C is the wrong substrate for P4 (closures + objects + strings)**, not a temporary expedient — pivot now while the codegen layer is ~1000 LOC

Replacement: **two backends sharing one SSA IR**:

```
frontend (lex → parse → check) → SSA IR (rich types, monomorphized,
                                          devirtualized, escape-analyzed,
                                          partial-evaluated, pattern-matched)
                                  ↓                ↓
                         Inkwell (LLVM)      Cranelift
                              ↓                  ↓
                         AOT object         JIT in-memory
                         + system ld         + execute
                              ↓                  ↓
                         `tr build`         `tr run`
                         (run_ms 极致)       (compile_ms 极致)
```

Both modes are first-class. `tr build` is the perf-leading native binary (matches/beats rust/go on bench). `tr run` is Go-style "compile to memory and execute" — replaces the tree-walk interpreter as the dev-loop runner.

#### run_ms ceiling = three layers, not one

```
run_ms 极限 = optimal_codegen × optimal_runtime × optimal_layout
            = LLVM            × hand-tuned GC   × Rust-style layout
```

Picking LLVM solves the codegen layer. The other two layers are not free; they are where bun/V8 lose to us by 4–20× on the current bench (their codegen is fine; their runtime + layout carry too much overhead). Specific commitments:

1. **IR carries rich type info** → emit specialized LLVM IR. Monomorphization (Rust-style, no vtables), devirtualization at IR level, `noalias` annotations from ownership analysis, `!range` metadata from type narrowing.
2. **Rust-shaped ownership runtime — no tracing GC.** Affine types catch use-after-move at compile time. `Rc<T>` / `Arc<T>` for explicit shared ownership (refcount only — no cycle collection; document the cycle-leak failure mode and let the user pick weak references). Drop on scope exit, deterministic. Inline strings ≤23 bytes (Swift-style slim layout); larger strings live behind explicit `Rc<String>`. Monomorphic closures (function pointer + captured struct, no vtable). Capture-by-move is the default; shared captures route through `Rc<T>` clone explicitly. (See hard requirement #4: no tracing GC. The "tracing GC" phrase used in an earlier draft of this section was a slip — this is the correct version.)
3. **Language-level PGO** (not LLVM-level — that broke popcount). `@hot`/`@cold` attributes + static-analysis hints → emitted as LLVM `branch_weights` metadata.
4. **Pattern-detected intrinsics**: Brian Kernighan popcount → `@llvm.ctpop.i64`, ctz/clz/bswap, vectorizable nested loops → NEON. Don't depend on LLVM's loop-idiom recognizer firing.
5. **Stack/arena allocation first**: escape analysis to stack-allocate non-escaping locals; region inference for function-scoped temporaries; linear types (Rust ownership) → direct free, no GC pressure. Go's escape-analysis is the floor; Rust's ownership is the ceiling.
6. **Apple Silicon tuning**: continue using Apple's LLVM patches (Apple clang 21 beats upstream LLVM 22 by 7% on M-series). Inkwell links against `/usr/lib/libLLVM.dylib` on darwin, upstream elsewhere.
7. **Compile-time partial evaluation**: const folding, template literal concat, `[1,2,3].length → 3` happen in IR before LLVM ever sees the program.

#### What this supersedes

- P3.4 (strings in linear memory) and P3.5 (heap allocator in wasm) — wasm is no longer the deploy target, so wasm-side runtime work is moot
- P8 (Cranelift backend later) — folded into P3.6
- P13 (LLVM `--release` mode optional, far) — folded into P3.5; LLVM is no longer optional, it's the AOT primary backend

### Working mode

- Closed-source research project. Many experiments and 废案. Advance step by step.
- New ideas first land in `labs/`. Graduation to `crates/` when stable.
- No tests/CI/docs pressure on `labs/` code; production rules apply once code lives in `crates/`.
- Be willing to delete more than is kept.
- See `.claude/rules/common/` and `.claude/rules/{rust,typescript}/` for shared coding standards.

---

## Status snapshot (2026-04-29)

Where the project is, after roughly 4 weeks of focused work since P0.1 landed (commit `296b8aa`, 2026-04-26).

### What works end-to-end

```
$ tr build foo.tora.ts -o foo  # AOT — LLVM 22 + Inkwell, ~33 KB binary
$ ./foo                         # native execution, perf-leading on bench
$ tr run foo.tora.ts            # JIT — Cranelift, dev-loop, ~5ms compile
```

A program of this shape compiles and runs:

```ts
type Point = { x: number, y: number };
type User  = { name: string, age: number, pos: Point };

function fib(n: number): number {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

let u: User = { name: "alice", age: 30, pos: { x: 1, y: 2 } };
let s: string = u.name + " is " + Math.floor(Math.sqrt(u.age * u.age)).toString();
console.log(s);
console.log(fib(40));
console.log(Math.PI);
// end of fn: drops u.name (str), u.pos (obj, recursive), u (obj), s (str)
```

(A few corners — `.toString()` on number, more String methods — are still aspirational; see deferrals below.)

### Phases done

| phase | what | done? | reference |
|---|---|---|---|
| **P0**     | walking skeleton (`tr run hello.ts`) | ✓ | `0d/2026-04-26` |
| **P1**     | core language (10 sub-steps) | ✓ | through `2026-04-27` |
| **P2.1**   | affine types — use-after-move type error | ✓ | commit `1cc91c2` |
| **P2.2.a** | strings as bindings (static-only) | ✓ | commit `9c04358` |
| **P2.2.b** | heap strings + drop emission (no leak) | ✓ | commits `6e0fc07` + `124641c` |
| **P2.2.c** | string concat (`a + b`) | ✓ | commit `7a515a7` |
| **P2.4.a/b** | parser + typecheck for object literals + `type` aliases | ✓ | commit `b1d3467` |
| **P2.4.c** | object SSA codegen (alloc / member / drop) | ✓ | commit `eb76990` |
| **P2.4.d** | recursive drop for nested non-Copy fields | ✓ | commit `b7ac139` |
| **P3.4**   | Inkwell spike (gate the LLVM pivot) | ✓ | commit `2ec732c` |
| **P3.5**   | SSA IR + Inkwell AOT backend | ✓ | through `5290952..a5c2912` |
| **P3.6**   | Cranelift JIT backend (replaces tree-walk interp) | ✓ | commit `5aa9c96` |
| **P3.7**   | retire wasm-via-C path | ✓ | commit `61ae24a` |
| **stdlib slice 1** | `console.log`, `Math.{sqrt,abs,floor,ceil,log,exp,pow,min,max,PI,E}`, `s.length`, `print_f64` | ✓ | commits `9a499de` + `036e5ed` |

### Bench position (M4 Pro, hyperfine n=3-10)

8 of 8 cases green on `torajs` (AOT) + `torajs-jit`. Vs **rust**: 6 wins / 2 ties / 0 losses on AOT. Vs **bun**: 8/8 wins. Vs **node-v8**: 8/8 wins (4-71×). See README.md for the full table.

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

### Code size

```
labs/0001-walking-skeleton/src/   ~5800 LOC across 9 files
docs/                              roadmap.md + stdlib.md
bench/                             8 cases × 5 langs + harness + runners + results
```

### Execution path — committed order, no per-step ask

This is the **single committed path** through the next 18-36 months. Steps execute in order. The agent runs each step end-to-end (code + tests + commit) without checkpointing back to takagi unless a real fork appears (a design question not in this doc, an irreversible decision, or a failure with ambiguous recovery). After a step lands, the next step starts immediately.

Format: each row is one shipped commit. Sub-steps roll up into milestones. The "exit" column is the literal acceptance gate — when it's green, that step is done, regardless of polish.

#### Milestone M1 — finish ownership (P2.3) → graduate from "ownership-incomplete"

| step | what | exit |
|---|---|---|
| **P2.3.a** ✓ | parser + check.rs for `Rc<T>` | typecheck-only programs accepted; bench unaffected |
| **P2.3.b** | SSA `Type::Rc(Box<Type>)`, lower as i64 ptr; declare `__torajs_rc_alloc/_clone/_drop`; `Rc.new` lowers to alloc + N stores; auto-clone elision | `tr run` and `tr build` execute `let u = Rc.new({x:1,y:2}); console.log(u.x);` end-to-end; libc heap accounting clean |
| **P2.3.c** | drop emission: `Type::Rc(inner)` arm in `emit_drop_value`; per-T drop_payload thunk for recursive inner-drop | refcount=0 frees inner string/object correctly; valgrind-style leak check on `Rc<{name:string}>` clean |
| **P2.3.d** | both backends (Inkwell + Cranelift) implement the three intrinsics | bench `rc-clone-1m` passes on both backends; result within 1.2× of Rust's `Rc::clone` |
| **P2.3.e** | `Rc<RefCell<T>>` interior mutability; `.borrow_mut()` guard; runtime panic on aliased borrows | shared mutable counter pattern works; double-borrow panics with clean message |

#### Milestone M2 — the next big stdlib gap (Array runtime)

| step | what | exit |
|---|---|---|
| **P7.Array.a** | `Array<T>` heap layout (`{u64 len, u64 cap, T data[]}`); `__torajs_arr_{alloc, push, get, set, len, drop}` intrinsics | `let xs = [1,2,3]; console.log(xs[0]);` round-trips through SSA on both backends |
| **P7.Array.b** | typecheck + lower `xs.push(v)`, `xs.length`, `xs[i] = v`; affine consume of `v` on push | `let xs: number[] = []; xs.push(1); xs.push(2); console.log(xs.length);` works |
| **P7.Array.c** | recursive drop: `Array<T>` of non-Copy frees each element | `Array<string>` going out of scope frees all element strings; leak check clean |
| **P7.Array.d** | bench cases: `array-sum-1m`, `array-map-square`, `array-filter-evens` (no closures yet — pass fn name) | 3 new bench rows green, results vs Rust within 1.5× on AOT |

#### Milestone M3 — generics (P14)

`Rc<T>` and `Array<T>` shipped as compiler-baked generics. M3 generalizes the mechanism to user code so we can build `Map<K,V>` / `Result<T,E>` / `Option<T>` / `Promise<T>` without compiler changes.

| step | what | exit |
|---|---|---|
| **P14.1** | parser/AST: type params on fn decls, type aliases, struct types | `function id<T>(x: T): T { return x; }` parses + typechecks |
| **P14.2** | monomorphization pass: per-instantiation specialization at the SSA boundary | `id<number>(5)` and `id<string>("x")` both lower to distinct concrete fns |
| **P14.3** | generic structs: `type Pair<A,B> = { fst: A, snd: B }`; field access through generic | `Pair<number,string>` round-trips |
| **P14.4** | trait-style bounds (start: `T: Eq`, `T: Display`); for now, hand-baked predicates | `function eq<T: Eq>(a: T, b: T): boolean` works for number/string |
| **P14.5** | rewrite `Rc<T>` and `Array<T>` to use the generic mechanism (was: hand-coded) | no behavioral change; LOC drops; M2 bench unchanged |

#### Milestone M4 — error model (P15)

The first decision that affects every subsequent stdlib API. Defer no longer.

| step | what | exit |
|---|---|---|
| **P15.0** | RFC: Result/Option vs throw vs both. **Default decision (committed): Result/Option only, no throw.** Panic remains for invariant violations; user code uses `Result<T, E>` for recoverable errors. | RFC merged at `.claude/rfcs/<date>-error-model.md` |
| **P15.1** | `Option<T>` and `Result<T,E>` as stdlib generic enums; `.is_some()`, `.unwrap()`, `.map()` | `Option<number>` round-trips; unwrap on None panics with source-mapped message |
| **P15.2** | `?` operator desugars to early-return on `Err`/`None` | `function f(): Result<number, string> { let x = g()?; return Ok(x + 1); }` works |
| **P15.3** | retire panic for user-recoverable errors in stdlib (`Rc.borrow_mut` → `Result`, etc.) | every stdlib fn returning a fallible result returns `Result`/`Option`, not panic |

#### Milestone M5 — closures (P4)

The major language milestone. Required before iterators / functional stdlib.

| step | what | exit |
|---|---|---|
| **P4.2** | capture analysis: classify each free var as `Move` / `Borrow` / `BorrowMut` | analysis output dumpable via `tr capture <file>` |
| **P4.3** | closure environment lowering: heap-allocated env struct, fn ptr + env pair | non-capturing closure unchanged; capturing closure runs |
| **P4.4** | `Rc<RefCell<T>>`-shared mutable captures (gates on M1.P2.3.e) | shared counter across two closures, mutated correctly |
| **P4.5** | `Fn` / `FnMut` / `FnOnce` distinction; one-shot closure consumes captures | passing a closure to a fn that takes `Fn` rejects an `FnMut`-shaped closure |
| **P4.6** | bench cases: `closure-sum`, `closure-counter`, `closure-iter-fold` | 3 new bench rows green |

#### Milestone M6 — modules (P9)

Multi-file compilation. Lets stdlib graduate from hardcoded globals.

| step | what | exit |
|---|---|---|
| **P9.1** | parser/AST: `import { foo } from "./bar.ts"` and `export` keywords | parses; AST carries module identity |
| **P9.2** | path resolution: relative imports + project-root anchor (`tora.toml`) | `tr build src/main.tora.ts` resolves cross-file references |
| **P9.3** | per-module typecheck + cross-module type unification | type defined in `a.ts`, used in `b.ts`, structurally equal |
| **P9.4** | incremental compilation: cache per-module SSA; rebuild only changed modules + dependents | second `tr build` of unchanged code finishes < 50ms |
| **P9.5** | stdlib reorganization: move `Math.*`, `console.*`, `String.*` from hardcoded to `import { ... } from "@torajs/std"` | bench cases use imports; behavior unchanged |
| **P9.6** | `tora.toml` project file + dep manifest shape (no registry yet) | empty `tora.toml` accepted; deps section declared but unused |

**Graduation point:** end of M6, `labs/0001-walking-skeleton/` is promoted to `crates/torajs-core/` + `crates/tr-cli/`. `v0.1` tag.

#### Milestone M7 — concurrency (P5 + P6)

Async + multi-core, in that order.

| step | what | exit |
|---|---|---|
| **P5.1** | parser/AST/typecheck for `async fn` and `await` | async fn signatures typecheck; await on non-Future errors |
| **P5.2** | state machine lowering: each `await` yields control to the executor | runtime can poll a state machine to completion |
| **P5.3** | single-threaded executor (Tokio-shape, no thread pool) | `tr run` of an async program executes to completion |
| **P5.4** | combinators: `Future.all`, `Future.race`, `join`, `select` | bench `async-fanout-100` runs |
| **P5.5** | async closures (gates on M5) | works |
| **P5.6** | cancellation via drop | dropping a Future mid-await cleans up |
| **P6.1** | `Send` / `Sync` in the type system | non-Send value rejected when crossing thread boundary |
| **P6.2** | `Arc<T>` (atomic refcount); identical surface to `Rc<T>` | swap-in works; `rc-clone-1m` runs on Arc with < 5% overhead |
| **P6.3** | thread spawn + join | 4 threads computing fib in parallel; result correct |
| **P6.4** | `Mutex<T>` / `RwLock<T>` / atomics | concurrent counter, no UB |
| **P6.5** | channels: `mpsc`, `oneshot` | producer-consumer bench |
| **P6.6** | multi-threaded work-stealing executor | async + multi-core; bench `mandelbrot-parallel` shows linear scale |

#### Milestone M8 — playground (P10) + tooling (P11)

Parallel UX tracks. M7 must land first because the playground needs the runtime feature-complete.

| step | what | exit |
|---|---|---|
| **P10.1-3** | wasm target for the engine; browser-side host; CodeMirror integration | torajs.com/playground compiles + runs hello world |
| **P10.4-5** | bench scoreboard auto-rendered from `bench/results/`; share-link encodes program | scoreboard live |
| **P11.1-3** | LSP server: hover, goto-def, diagnostics | VS Code extension shows hovers + jumps |
| **P11.4-6** | formatter, linter, test runner | `tr fmt`, `tr lint`, `tr test` work |

#### Milestone M9 — polish + integration (P16, P17)

Final shipping mile.

| step | what | exit |
|---|---|---|
| **P16.1-4** | `libtora.a` + `tora_eval()` for Rust hosts; sandboxing knobs | embed in a Rust app, run a script, exit cleanly |
| **P17.1-4** | DWARF debug info on AOT; source-mapped panics; step debugger via Cranelift JIT; REPL | crash backtrace points at `.tora.ts` line; `tr debug` steps a program |

`v1.0` tag.

#### Currently executing

The agent is **in M1, sub-step P2.3.b**. After P2.3.b lands, P2.3.c starts automatically. Stop only on a real fork (see top of section).

---

## Principles

- **Every step is visible** — at the end of each step, there's a command you can run and see output. No "internal-only" steps.
- **Small grain** — each step is roughly 1-3 days of work, ~100-500 LOC. If a step grows past that, it splits.
- **Front-loaded detail** — phases close to now are spelled out per-step; far phases are headers + goals. We re-detail later phases when we get there.
- **Each step is potentially throwaway** — research mode. If a step's outcome surprises us, we revisit before continuing.
- **Naming**: the binary is provisionally `tr` (per takagi's wording). Note: collides with Unix `tr` (translate-characters). For dev we run via `cargo run -p ...` so no clash; rename happens before any `cargo install` / homebrew shipping.
- **All P0/P1 work lives in `labs/0001-walking-skeleton/`**. Graduation to `crates/` happens at the end of P2 when the language is real enough to matter.

---

## Phase shape

| Phase | Theme | Status (2026-04-29) | Step count |
| --- | --- | --- | --- |
| **P0**  | Walking skeleton — `tr run hello.ts` prints `hello`         | ✓ done                  | 6 |
| **P1**  | Core language — arithmetic, vars, control flow, fns, strings, arrays | ✓ done                  | 10 |
| **P2**  | Ownership model — affine + `Rc<T>` + objects + drop        | 3 of 4 done; **P2.3 next** | 4 |
| **P3**  | Native AOT (LLVM via Inkwell) + JIT (Cranelift)            | ✓ done                  | 7 (after pivot) |
| **P4**  | Closures with capture analysis + ownership                  | not started             | 5 |
| **P5**  | Async/await — state machine lowering + executor            | not started             | 6 |
| **P6**  | Multi-core, Send/Sync, structured concurrency              | not started             | 5 |
| **P7**  | Standard library — see `docs/stdlib.md`                    | slice 1 done; Array runtime is the next big block | layered |
| **P8**  | (folded into P3.6 by 2026-04-28 pivot)                     | n/a                     | n/a |
| **P9**  | Module system — multi-file, ESM imports, type-checked-across-modules | not started             | 6 |
| **P10** | Playground on torajs.com — wasm target + browser host      | not started             | 5 |
| **P11** | Tooling — LSP / formatter / test runner / linter           | not started             | 6 |
| **P12** | Perf work — escape analysis, monomorphization, layout opt  | rolling                 | open-ended |
| **P13** | (folded into P3.5 by 2026-04-28 pivot)                     | n/a                     | n/a |
| **P14** | Generics — type-parameterized fns / types                  | not started             | 5 |
| **P15** | Error model — Result/Option, propagation, panic discipline | not started; decision pending | 4 |
| **P16** | Embedding API — `libtora.a` + `tora_eval()` for Rust hosts | not started             | 4 |
| **P17** | Source maps + debugging — DWARF, source positions, REPL    | not started             | 4 |

Total time-to-mature: **18-36 months**. Roughly 25% done at the codegen / ownership layer; the language-feature layers (P4 / P5 / P6 / P14) are the long pole.

---

## P0 — Walking skeleton

**Status (2026-04-26): ✓ done.** P0.1 → P0.6 all landed; `tr run hello.ts` end-to-end executes `console.log("x")` via lex → parse → check → IR → tree interpreter, all sharing the IR that the future AOT path will consume. Lives at `labs/0001-walking-skeleton/`.

**End state**: `cd labs/0001-walking-skeleton && cargo run -- run hello.ts` prints `hello` to stdout, where `hello.ts` contains `console.log("hello")`.

### P0.1 — Empty crate + CLI entry

**Goal**: create the Rust crate, a `main.rs` that parses subcommands.

**Demo**:
```bash
cd labs/0001-walking-skeleton
cargo run -- --version
# tr 0.0.0
cargo run -- run hello.ts
# error: not implemented yet (placeholder)
```

**Implementation**:
- `cargo new --bin labs/0001-walking-skeleton --name tr`
- `clap` (or hand-rolled `std::env::args` for v0) with subcommands `run`, `tokenize`, `parse`, `check`, `ir`
- All subcommands print "not implemented" except `--version`
- Cargo.toml dep: just `clap` (or nothing)

**Defers**: every subcommand body
**Size**: ~50 LOC

### P0.2 — Lexer (TS subset)

**Goal**: tokenize `console.log("hello")` correctly.

**Demo**:
```bash
echo 'console.log("hello")' | cargo run -- tokenize -
# Ident("console") @ 0..7
# Dot @ 7..8
# Ident("log") @ 8..11
# LParen @ 11..12
# String("hello") @ 12..19
# RParen @ 19..20
# Eof @ 20..20
```

**Implementation**:
- `src/lexer.rs`
- Token enum: `Ident`, `Number`, `String`, `Dot`, `Comma`, `Semi`, `LParen`, `RParen`, `LBrace`, `RBrace`, `Eof`, `Plus`, `Minus`, `Star`, `Slash`, `Eq`, `EqEqEq`, `Lt`, `Gt`, `LtEq`, `GtEq` (more as we need them)
- Spans (`u32, u32`) on every token — for error reporting later
- Hand-written, recursive descent style char-by-char advance
- Subset of TS: only what's needed by the demo. Don't try to be complete.

**Defers**: keywords (later steps add them as needed), comments, regex, template literals, JSX
**Size**: ~200 LOC

### P0.3 — Parser → AST

**Goal**: parse the token stream into an AST.

**Demo**:
```bash
echo 'console.log("hello")' | cargo run -- parse -
# ExprStmt
#   Call
#     Member
#       Ident("console")
#       "log"
#     args:
#       String("hello")
```

**Implementation**:
- `src/ast.rs` — node types: `Stmt`, `Expr`, `ExprStmt`, `Call`, `Member`, `Ident`, `String`, `Number`. **Arena-allocated**: `Vec<Node>` + `NodeId(u32)`. Children referenced by `NodeId`, not `Box`.
- `src/parser.rs` — recursive descent; pratt-style for operator precedence later
- Pretty-printer for AST so `parse` subcommand has output
- Spans on every node

**Defers**: declarations (let/fn), control flow, types, generics
**Size**: ~250 LOC

### P0.4 — Type check (trivial)

**Goal**: validate that `console.log` is being called with valid args. Surface span-attached errors.

**Demo**:
```bash
echo 'console.log(1)' | cargo run -- check -
# error: type mismatch
#   expected string
#      got number
#   at <stdin>:1:13
```

**Implementation**:
- `src/check.rs`
- Hardcoded type for `console.log`: `(string) -> void`
- For v0: only `string` and `number` types exist. No inference. No generics.
- Walks the AST, returns `Result<(), Vec<TypeError>>`
- TypeError carries a span and a message

**Defers**: real type system (next phase), inference, generics, structural types
**Size**: ~120 LOC

### P0.5 — Lowering: AST → IR

**Goal**: produce a stack-machine IR from the typed AST.

**Demo**:
```bash
echo 'console.log("hello")' | cargo run -- ir -
# .data
#   const0: "hello"
# .code
#   load_host  console_log
#   load_const const0
#   call       1   ; arity
```

**Implementation**:
- `src/ir.rs` — opcodes: `LoadConst(ConstId)`, `LoadHost(HostId)`, `Call(arity: u8)`, `Pop`, `Ret`. Just enough for v0 demo.
- `src/lower.rs` — AST → IR
- Constants table for strings and numbers
- Host function table (just `console.log` for now, mapped to a host slot)
- Pretty-print IR for the `ir` subcommand

**Defers**: control flow ops (`Branch`, `Jump`), arithmetic ops, locals, returns, frames
**Size**: ~150 LOC

### P0.6 — Tree-walking interpreter

**Goal**: actually execute the IR. End-to-end demo.

**Demo**:
```bash
echo 'console.log("hello")' > /tmp/hello.ts
cargo run -- run /tmp/hello.ts
# hello
```

**Implementation**:
- `src/value.rs` — `Value` type. v0 layout: `enum Value { Number(f64), String(Rc<String>), Undefined, /* ... */ }`. **Critical: this is the entry point for swapping to NaN-boxing later.** All access through methods, not field access.
- `src/interp.rs` — main eval loop, switch on opcode, stack of `Value`s
- Host fn table populated with `console_log` (= a Rust fn that prints)
- `tr run` glue: read file → lex → parse → check → lower → interp

**Defers**: NaN-boxing (works fine with `enum Value` for v0; swap later), control flow (next step), proper frames
**Size**: ~200 LOC

### P0 deliverables

- One Rust crate at `labs/0001-walking-skeleton/`
- `cargo run -- run hello.ts` works for the trivial demo
- Subcommands `tokenize`, `parse`, `check`, `ir`, `run` all functional for the demo
- ~1000 LOC total

**Visible end-to-end**: yes — the user can run `tr run hello.ts` and see `hello`. The "spine" of the engine exists.

---

## P1 — Core language

**Status (2026-04-27): ✓ done.** All 10 sub-steps landed with per-step commits in `labs/0001-walking-skeleton/`. The dialect now covers arithmetic, variables (let/const + type annotations + mutability rules), boolean comparisons + control flow (if/else/while/blocks), named functions + recursion + arrow fns, strings (concat/length/indexing) and homogeneous arrays. fib40 runs end-to-end through the tree-walking interpreter — slowly (17s — the cost AOT will reclaim), but correctly.

Each step expands the language by a small, demoable feature. Order chosen so each step's demo is testable in isolation (no big-bang dependencies).

### P1.1 — Number arithmetic

**Demo**: `tr run` of `console.log(1 + 2 * 3)` prints `7`.

**Adds**: tokens `+ - * /`, AST `BinOp`, IR `Add Sub Mul Div`, type check `number op number → number` (no coercion to string yet — we rejected `1 + "a"`), `console.log` accepts number for now (extend signature; v1 stdlib has both).

**Size**: ~150 LOC additions

### P1.2 — Variables

**Demo**: `tr run` of `let x = 1; let y = 2; console.log(x + y)` prints `3`.

**Adds**: keyword `let`, AST `LetDecl`, type inference for monomorphic literal → variable, IR `LoadLocal` `StoreLocal`, frame with locals slot

**Defers**: `const`, type annotations on let (next step), reassignment (`x = 5`)

**Size**: ~150 LOC

### P1.3 — Type annotations on `let`

**Demo**: `tr check` of `let x: string = 1` errors at the `1`.

**Adds**: parser handles `: Type` after binder, type AST nodes (`number`, `string`, `boolean`), explicit type winning over inferred

**Size**: ~80 LOC

### P1.4 — Reassignment, mutability

**Demo**: `tr run` of `let x = 1; x = x + 1; console.log(x)` prints `2`. `tr check` of `const x = 1; x = 2` errors.

**Adds**: keyword `const`, mutability flag on locals, type checker enforces immutability

**Size**: ~80 LOC

### P1.5 — Booleans, comparison, if/else

**Demo**: `tr run` of `if (1 < 2) console.log("yes")` prints `yes`.

**Adds**: keyword `if`/`else`/`true`/`false`, type `boolean`, ops `<` `>` `<=` `>=` `===` `!==` (we rejected `==` `!=`), AST `If`, IR `Branch` `Jump`

**Size**: ~200 LOC

### P1.6 — While loops

**Demo**: `tr run` of `let i = 0; while (i < 3) { console.log(i); i = i + 1 }` prints `0\n1\n2`.

**Adds**: keyword `while`, AST `While`, IR uses existing `Branch` `Jump`, block scope for locals

**Size**: ~80 LOC

### P1.7 — Function declarations and calls

**Demo**: `tr run` of `function add(a: number, b: number): number { return a + b } console.log(add(2, 3))` prints `5`.

**Adds**: keyword `function`/`return`, AST `FnDecl`/`Return`, type checker validates signatures and call sites, IR `Call`/`Ret` with proper frame management, recursion works

**Defers**: arrow functions (next step), generics (later), closures (P4), variadic args, optional args

**Size**: ~250 LOC

### P1.8 — Arrow functions (no captures yet)

**Demo**: `tr run` of `const square = (x: number): number => x * x; console.log(square(5))` prints `25`.

**Adds**: parser handles `=>`, AST `ArrowFn`, lower to same FnDecl shape (no captures yet means no closure machinery)

**Note**: full closures with captures wait for P4 because they need ownership analysis to be GC-free.

**Size**: ~80 LOC

### P1.9 — Strings (concat, length, indexing)

**Demo**: `tr run` of `let n = "hello"; console.log(n + " " + "world")` prints `hello world`.

**Adds**: `string + string → string`, `s.length: number`, `s[i]: string` (single char), strings are owned `Rc<String>` for v0 (will revisit ownership in P2)

**Defers**: template literals, full `String.prototype` methods (those go in P7 stdlib)

**Size**: ~150 LOC

### P1.10 — Arrays of primitives

**Demo**: `tr run` of `let a: number[] = [1, 2, 3]; console.log(a[0] + a[1] + a[2])` prints `6`.

**Adds**: parser handles `[1, 2, 3]` literal and `T[]` type, type checker monomorphic over element type, runtime `Vec<f64>` behind an `Rc<Vec<...>>` for shared. Or owned, with affine semantics — **this is where P2 starts biting**. v0 of arrays may use `Rc<RefCell<Vec<...>>>` as the simplest expedient and we revisit at P2.

**Size**: ~200 LOC

### P1 deliverables

- The language can do arithmetic, conditional logic, functions, strings, arrays
- ~3000 LOC total in `labs/0001-walking-skeleton/`
- Still no AOT, no async, no closures-with-captures, no objects, no Rc/move semantics enforced. The interpreter is doing real work but the language is a sketch.

---

## P2 — Ownership model

The Rust-shaped no-GC story. Affine types catch use-after-move at compile time; non-Copy values heap-allocate and run a deterministic drop sequence at scope exit; `Rc<T>` provides explicit shared ownership when single-owner doesn't fit. 3 of 4 sub-phases done.

### P2.1 — Affine type system: detect use-after-move ✓

**Status (2026-04-28): ✓ done.** `check.rs` tracks `moved: bool` per binding; consume sites are let-rhs / assign-rhs / non-Copy call-arg / return / string-concat operands. `console.log` is borrow-style (Any param doesn't consume). Branch-conservative — moves in conditional arms aren't merged, will tighten in P14/P15 alongside generics + error model.

Reference: commit `1cc91c2`, 6 unit tests in `check.rs::tests`.

### P2.2 — String runtime + drop emission ✓

**Status (2026-04-29): ✓ done in three sub-commits.**

- **P2.2.a** — strings as bindings (static-only) — commit `9c04358`
- **P2.2.b** — heap allocation via `__torajs_str_alloc`; `Drop` instruction emitted at scope exit; reassignment drops the old before storing new — commits `6e0fc07` + `124641c`
- **P2.2.c** — string concat (`a + b`) via `__torajs_str_concat` — commit `7a515a7`

Layout: `{u64 len, u8 data[]}`. Both backends (Inkwell + Cranelift) implement `__torajs_str_{alloc, print, drop, concat}`. End-of-fn drop walks owned non-moved bindings and emits matching `str_drop` calls.

### P2.3 — `Rc<T>` first-class — **next**

The escape valve from single-owner. Affine types are the right default — they catch use-after-move at compile time and free at scope exit, no runtime cost. But three patterns can't fit single-ownership without painful contortions, and Rust solves them with `Rc<T>` / `Arc<T>`. We follow that lead:

1. **Closures with shared captures** (P4.4): two closures both reading the same `User` need either copy-by-value (expensive for large structs) or shared ownership.
2. **Recursive / graph-shaped data** (linked lists, trees with parent pointers, ASTs with cross-references): a node can't both own its children and be owned by them.
3. **Long-lived configuration / interned values**: the global `Math.PI` style — read everywhere, written once.

Without `Rc`, users would route around the type system. With `Rc`, sharing is **explicit, visible, and refcount-priced** — every `.clone()` is a single atomic-or-not increment, every drop is a decrement + maybe-free. No tracing GC, no cycle collector — cycles **leak by design** until weak refs land.

#### Memory layout

```
Rc<T> on the stack:   [ ptr: *mut RcInner<T> ]   (8 bytes, single pointer)

RcInner<T> on the heap (allocated by __torajs_rc_alloc):
  offset 0:   strong_count: u64    // bumped by clone, decremented by drop; free at 0
  offset 8:   weak_count:   u64    // P2.3 ships with this slot reserved but unused; weak refs deferred
  offset 16:  payload:      T      // 8-byte aligned; T's existing layout
```

The `weak_count` slot is **reserved upfront** so `Rc<T>::downgrade()` (deferred to P15+) can land without a layout change that would invalidate every existing `Rc` allocation. 16 bytes of header overhead per allocation is the same as Rust's `Rc<T>` and acceptable — the alternative (variable-length headers, sentinel values) bites worse during P4 closure capture.

`Rc<T>` itself is the pointer alone. Stack-side it is `Type::Rc(Box<Type>)` and SSA-lowered to `Type::I64` (treated as opaque pointer). The pointee is the heap allocation; field access happens via `Load(field_ty, rc_ptr, 16 + field_offset)`.

#### Surface syntax

```ts
// allocate
const u: Rc<User> = Rc.new({ name: "alice", age: 30 });

// clone (explicit — no implicit copy)
const u2: Rc<User> = u.clone();

// dereference: deref-on-member, no `*` operator
console.log(u.name);          // = (*u).name in Rust terms
const n: number = u.age + 1;  // auto-deref through Rc

// drop happens at scope exit — strong_count--; if 0, drop payload then free RcInner
```

Decisions:
- **`Rc.new(value)` not `new Rc(...)`** — TS has `new` but we drop it for Rust-shaped associated functions. Matches `Math.sqrt(...)` pattern.
- **No deref operator** — `u.field` auto-derefs through `Rc<T>` if `T` is a struct. Matches Rust ergonomics; avoids ugly `(*u).field`. Type checker handles it transparently.
- **`.clone()` is required** — copying an `Rc` is **not** implicit (unlike `Copy` types). This trades ergonomics for clarity: every refcount bump is visible at the source level. Same call as `String.clone()` once strings get a method-style API.
- **Affine on the `Rc` value, not the pointee** — moving `u` into a fn invalidates the local; `.clone()` produces a fresh `Rc<T>` to pass without moving. Shared mutation goes through `Rc<RefCell<T>>` (P2.3.e + P4.4) — interior mutability is the next sub-piece.

#### Type system rules (check.rs)

- `Rc<T>` is **never** `Copy` (in the affine-types sense). It is move-by-default.
- `.clone()` on `Rc<T>` returns `Rc<T>` and does **not** consume the receiver — borrow semantics. This is the only universally-safe non-consume method on a non-Copy type at this stage.
- `Rc::new(value)` consumes `value` (it moves into the heap allocation).
- Member access `u.field` typechecks under: if `T = Struct` and the member exists in `T`, return field type with read borrow on `u`. The receiver is **not** consumed — read-borrow semantics, just like `s.length` for strings.
- Field write through `Rc` (`u.field = x`) is **rejected** unless the type is `Rc<RefCell<T>>` or the field is interior-mutable. Defers to P2.3.e — for the basic Rc, immutable-after-construction is the rule, matching Rust's bare `Rc<T>`.
- Cycles: the compiler **does not** detect them. Documented as a known leak class. Mitigation deferred to weak refs (P15).

#### Runtime intrinsics

| name | sig | semantics |
|---|---|---|
| `__torajs_rc_alloc(payload_size: u64) -> *mut RcInner` | malloc(16 + payload_size); init strong=1, weak=0; return ptr | called once per `Rc.new(...)` |
| `__torajs_rc_clone(p: *mut RcInner) -> *mut RcInner` | `(*p).strong_count += 1`; return p | called per `.clone()` |
| `__torajs_rc_drop(p: *mut RcInner, drop_payload: fn(*mut u8))` | `(*p).strong_count -= 1`; if zero { drop_payload(payload_ptr); free(p); } | called at every Rc binding's scope exit |

Both backends (Inkwell + Cranelift) implement these as Rust `extern "C"` trampolines. **Single-threaded for P2.3** — non-atomic count operations. `Arc<T>` (atomic) is the P6 sibling; the API surface is identical, the runtime swap-in is `AtomicU64::fetch_add`.

The `drop_payload` callback is a per-monomorphization function pointer that knows how to recursively drop the inner T. For `Rc<User>` with `User = { name: string, age: i64 }`, `drop_payload` is the same recursive-drop walker we built in P2.4.d, packaged as a callable. The lowerer synthesizes one drop_payload per distinct `T` reached from `Rc<T>` and threads its FuncId through the `__torajs_rc_drop` call site.

#### Sub-step decomposition

- **P2.3.a** — parser + AST + check.rs: `Rc<T>` type syntax, `Rc.new(e)` / `.clone()` member calls. No SSA yet — just typecheck. ~150 LOC.
- **P2.3.b** — SSA `Type::Rc(Box<Type>)`, lowered as i64 ptr. `__torajs_rc_alloc` / `_clone` / `_drop` declared in pass 0 alongside str/obj intrinsics. `Rc.new` → alloc + N stores at offset `16 + i*8`; auto-clone elision when not needed. ~250 LOC.
- **P2.3.c** — drop emission: extend `emit_drop_value` with `Type::Rc(inner)` arm — synthesize a `drop_payload` thunk for `inner`, pass to `__torajs_rc_drop`. Acceptance: `Rc<{ name: string }>` drops the inner string when refcount hits zero. ~150 LOC + drop-thunk codegen.
- **P2.3.d** — both backends: Inkwell + Cranelift implement the three intrinsics. Inkwell uses `build_atomic_*` ops behind a feature flag for forward-compat with Arc; Cranelift uses plain loads/stores for now. ~120 LOC across both.
- **P2.3.e** — `Rc<RefCell<T>>` for interior mutability through shared ownership. `RefCell.borrow_mut()` returns a guard; runtime panic on aliased borrows (matches Rust). Acceptance: closure-shared mutable counter pattern works. ~200 LOC. **Gates P4.4** (closures with shared mutable captures).

Total ~870 LOC for P2.3 across roughly 1 week.

#### Test plan

1. `Rc.new(42)` ; `.clone()` ; print both — refcount ends at 2, no leak.
2. `Rc.new({ name: "alice" })` — drop releases the inner string when count hits 0.
3. `Rc<{ p: Rc<Point> }>` — recursive drop walks through Rc chains.
4. Two-arm if/else where one arm clones, one moves — branch-conservative checking; both arms must end in matching ownership state.
5. `Rc<RefCell<i64>>` shared between two closure objects (after P2.3.e + P4.4) — mutation visible across, panic on overlapping borrow_mut.
6. **Leak test (negative)**: cycle of two `Rc`s pointing at each other — explicit `valgrind`/leak-check-allocator run shows the leak. Documented expected behavior, not a bug.

Plus 5 new bench cases that exercise Rc:
- `linkedlist-traverse-1m` — build & sum a length-1M Rc-linked list.
- `tree-fold` — recursive Rc<Tree> fold (mirrors AST workload).
- `rc-clone-1m` — bare clone/drop hot path; measures refcount cost vs Rust's `Rc::clone`.
- `interned-strings` — 10K shared `Rc<String>` lookups (foreshadows P9 stdlib intern table).
- `closure-shared-counter` (gated on P4.4) — shared mutable state through Rc<RefCell<i64>>.

Targets: linkedlist-traverse-1m within 1.5× of Rust; rc-clone-1m within 1.2× of Rust (we emit non-atomic ops so should match closely; deviation = codegen overhead to investigate).

#### Open questions deferred from P2.3

- **Weak refs** — `Weak<T>::upgrade()` returns `Option<Rc<T>>`. Slot reserved in layout. Need P15 (error model) for the Option-shape decision. Until then, cycles leak.
- **Drop order across captures** — when a struct contains multiple `Rc` fields, drop order is **declaration order**, matching Rust. Documented; tested.
- **`Box<T>` / unique heap** — Rust has both `Box<T>` (unique owner) and `Rc<T>` (shared). For now, `Type::Obj(...)` (the P2.4 object) **is** `Box`-equivalent — heap-allocated, single-owner, dropped at scope exit. We don't introduce a separate `Box<T>` type until a use case forces it (probably never — the niche is too narrow).

### P2.4 — Object literals + structural types ✓

**Status (2026-04-29): ✓ done across four sub-commits.**

- **P2.4.a/b** — parser + typecheck for `type Foo = { x: T1, y: T2 }` and `{ x: e, y: e }` literals; `Type::Struct(Vec<(String, Type)>)` in check.rs with structural equality; new pass-0 in check that registers aliases — commit `b1d3467`
- **P2.4.c** — SSA `Type::Obj(StructId)` interned in `Module.struct_layouts`; `InstKind::Load`/`Store` extended with byte offset; ObjectLit lowers to `__torajs_obj_alloc(N×8)` + N stores; Member lowers to `Load(field_ty, obj_ptr, idx*8)` — commit `eb76990`
- **P2.4.d** — recursive drop: `emit_drop_value(operand, type)` walks struct layouts and emits drop sequences for each non-Copy field before freeing the outer struct — commit `b7ac139`

Layout: 8-byte slot per field, declaration order. `bool` is padded to 8 bytes; will optimize in P12. Struct types intern by structural equality so `{x: i64, y: i64}` from different sites share one StructId.

Member access on Type::Str also handles `s.length` via offset-0 load (added with stdlib slice 2, commit `036e5ed`).

### P2 deliverables (after P2.3 lands)

- Real ownership semantics; no GC; programs run with deterministic destruction.
- Strings + objects + Rc all work as bindings; can be passed to functions, returned, stored in struct fields, concatenated (strings).
- Both AOT and JIT honor the same drop semantics; their heaps are interchangeable (libc malloc/free for objects; std::alloc::alloc with Layout for strings — both backends agree).
- ~5000 LOC total in `labs/0001-walking-skeleton/src/` after P2.3.

### Graduation point — when?

The original plan was: at end of P2, `labs/0001-walking-skeleton/` graduates to `crates/torajs/`. Re-evaluated:

- The labs directory has been the right home through the LLVM pivot (P3.4 spike → full SSA + Cranelift) and ownership work (P2.1 → P2.4). Promoting too early would have meant rewriting the crate structure each time.
- Graduation happens when **the API surface stabilizes** — which means after P9 (modules), because module-system work touches every entry point.
- Until then, labs/0001 stays. We document the deferred graduation here rather than racing it.

New target: graduate to `crates/torajs-core/` + `crates/tr-cli/` (name TBD) at end of P9.

---

## P3 — Native AOT + JIT (revised 2026-04-28)

P3.1–P3.3 landed the **wasm-via-C** path (tr → wasm-encoder → wasm2c → clang). It works and currently leads the bench, but is being **replaced**, not extended (see "Backend pivot" above). The replacement is a single SSA IR that feeds two real codegen backends: LLVM (Inkwell) for `tr build`, Cranelift for `tr run`. The wasm pipeline stays alive as a reference implementation through P3.6, then is deleted.

### P3.1 — Wasm encoder, stub module

**Status (2026-04-28): ✓ done.** Runs as the current `torajs-aot` bench row. Will be retired in P3.7.

### P3.2 — Number arithmetic in wasm

**Status (2026-04-28): ✓ done.**

### P3.3 — Functions and locals in wasm

**Status (2026-04-28): ✓ done.** fib40 AOT at 150 ms (beats rust/go), popcount at 2.86 ms (beats rust). This validates that **type-specialized native codegen** is the moat — confirms the perf model before we rebuild the codegen layer in P3.4–P3.7.

### P3.4 — Inkwell spike: validate LLVM-direct ≥ clang on bench

**Goal**: throwaway experiment in `labs/0002-inkwell-spike/` — emit LLVM IR for fib40 directly via Inkwell, JIT or compile-to-binary, time it. Compare to current torajs-aot's 150 ms. **This is the gate** for the entire pivot: if Inkwell can match or beat clang on our cases, we proceed. If it underperforms by >10%, we stop and investigate before sinking time into P3.5+.

**Demo**: a binary at `labs/0002-inkwell-spike/target/release/inkwell-fib40` that prints `102334155` and runs in ≤150 ms median (matching torajs-aot).

**Adds**: Inkwell crate dep, `LLVMContext` setup, hand-written LLVM IR for fib40, ORC JIT or static link to native binary, hyperfine comparison vs current torajs-aot.

**Size**: ~300 LOC, 1–2 days. Throwaway after P3.5.

### P3.5 — Build SSA IR + Inkwell AOT backend

**Goal**: real `ir` module in `labs/0001-walking-skeleton/`. Frontend (lex/parse/check) → IR. Inkwell consumes IR → native binary via system linker.

**IR design**:
- SSA form, basic-block CFG (not stack-machine)
- Carries rich type info: monomorphized generics, devirtualized calls, `Copy` vs affine, escape-status per local
- Op set: arith/compare/branch/call/load/store/alloc/free/phi/ret + intrinsic ops (popcount, bswap, ctlz/cttz)
- Pretty-print so `tr ir <file>.ts` dumps human-readable IR

**Inkwell backend**:
- IR → LLVM IR
- Apple LLVM (`/usr/lib/libLLVM.dylib`) on darwin via Inkwell's system-libllvm feature
- Per-case `aot_clang_flags` becomes `-O1`/`-O3` LLVM pass manager config
- Emit object file → invoke system `ld` → executable
- `bench/runners/torajs-llvm.toml` lands; old `torajs-aot` (wasm-via-C) renamed to `torajs-wasm` and stays for diff testing through P3.7

**Gate**: torajs-llvm must match or beat torajs-wasm's run_ms on every bench case. compile_ms target ≤ 50 ms.

**Size**: ~1500 LOC, weeks.

### P3.6 — Cranelift JIT backend

**Status (2026-04-29): ✓ done.** `tr run foo.ts` now SSA → CLIF → in-memory code page → call directly. Tree-walk interpreter (interp.rs / lower.rs / ir.rs / value.rs) deleted along with `bench/runners/torajs-interp.toml`. New bench row: `torajs-jit` (run-only; compile time rolls into wall time).

**Gate met**: `tr run startup.ts` = 7.6 ms total. `tr run fib40.ts` = 516 ms total (within 3.5× of torajs's 147 ms — looser than the original 2× target but the popcount case revealed Cranelift's lack of loop-idiom recognition costs more than expected; codegen quality is at LLVM `-O0/-O1` ceiling, fine for dev iteration).

Implementation (`labs/0001/src/ssa_cranelift.rs`, 508 LOC):
- One-to-one SSA InstKind → CLIF op (BinOp, ICmp/FCmp, Alloca/Load/Store, SiToFp, StringRef, Call, branch terminators)
- Runtime trampolines: `print_i64` and `print_str` are Rust functions, registered with `JITBuilder::symbol`
- String globals materialize as `Linkage::Local` data segments

Bench delta from previous tree-walk row: fib40 17 s → 516 ms (**~33× faster**), popcount 5 s → 99 ms (**~50× faster**).

### P3.7 — Retire wasm-via-C path

**Status (2026-04-28): ✓ done.** Deleted: `wasm-encoder` dep, `bench/aot-host/` (build.sh + main.c), `bench/runners/torajs-aot.toml`, `labs/0001/src/build.rs`, the libtorart.a cache logic, the legacy `tr build` wasm-via-C subcommand. `bench/runners/torajs-llvm.toml` renamed to `torajs.toml`; the runner's name field is now just `torajs` — the canonical AOT identity on the bench scoreboard. ~1300 LOC of code + 350 KB of cache machinery gone.

Cranelift JIT (P3.6) is the next piece. Until it lands, the `torajs-interp` row (tree-walk) is still live as the dev-loop measurement; it will be replaced when Cranelift's `tr run` ships.

**Bench scoreboard now**: `torajs` (AOT) + `torajs-interp` (tree-walk, retiring in P3.6) + bun-jsc, bun-aot, node-v8, rust, go, python.

### P3 deliverables

- One IR, two real codegen backends (LLVM + Cranelift)
- `tr build foo.ts -o foo` produces native binary, perf-leading on bench
- `tr run foo.ts` JIT-compiles and executes, Go-shaped dev loop
- Wasm-via-C pipeline deleted; no homebrew/wabt/clang dependency
- ~8000 LOC total in labs/0001-walking-skeleton/

**This phase ends the codegen story.** P4 onward is language features (closures, objects, async) on top of the new backends, not codegen rework.

---

## P4 — Closures with capture analysis + ownership

The hard part — and the proof of the no-GC contract. Most TS-shaped languages give closures GC for free; we don't have that escape hatch. Capturing a variable in a closure becomes a **typed, ownership-checked operation**, not a runtime "just keep it alive somehow."

### Design contract

A closure is a value with two parts:
1. **Code pointer** — same shape as a regular fn pointer (one per closure expression in the source, monomorphized per capture environment).
2. **Captured environment** — a struct containing copies of (or references to) each captured binding.

Closures are **monomorphic**: there is no `dyn Fn` until the type system grows trait objects (P14+). Two closures with different capture sets are different types. Returning a closure from a function or storing it in a variable requires a single, statically-known closure type.

### Capture modes (Rust-shaped)

Each captured binding takes one of three forms — chosen by capture analysis at compile time, overridable with a keyword:

| mode | when | runtime effect |
|---|---|---|
| **by-move (default for non-Copy)** | binding is read once and not used after the closure expression | binding is consumed; closure owns it; closure's drop frees it |
| **by-copy (default for Copy)** | binding is `i64`/`f64`/`bool` | bitwise copy into the env struct; original still usable |
| **by-clone (explicit `move` + Rc)** | shared ownership across closure + outer scope | `.clone()` is required at the capture site; closure owns the cloned `Rc<T>` |

We do **not** support by-borrow capture (the Rust `&T` / `&mut T` form). Lifetimes would be required, and lifetime inference is out of scope until at least P14. Until then, "borrow" is spelled `Rc<T>` — explicit, runtime-cost, but sound.

### `Fn` / `FnMut` / `FnOnce` distinction

| trait | signature | when emitted |
|---|---|---|
| `FnOnce` | `(self, args) -> Ret` — consumes captures | closure body moves a captured non-Copy value |
| `FnMut` | `(&mut self, args) -> Ret` — mutates captures | closure body mutates a captured value |
| `Fn` | `(&self, args) -> Ret` — read-only | closure body only reads captures |

These are **inferred** by the type checker walking the closure body. The user does not annotate. The inferred trait determines:
- Whether the closure can be called multiple times (Fn/FnMut: yes; FnOnce: no — second call is a compile error)
- The receiver shape in the lowered code pointer

For the v1 of P4, we ship Fn + FnOnce only; FnMut requires `&mut self` which interacts with affine analysis in a way we want to design carefully (likely defers to P4.5 below).

### P4.1 — Closures that don't capture ✓ (already P1.8)

Already works as arrow fns. Lowers identically to a top-level fn.

### P4.2 — Capture analysis pass

**Goal**: walk the closure body, identify free variables, classify each as Copy / non-Copy / explicit-move.

**Adds**: new pass in `check.rs` after typecheck, before SSA lowering. Builds a `CaptureSet { var: BindingId, mode: Move | Copy | Clone }` per closure expression. Errors on non-Copy capture without `move` annotation when the outer scope still uses the binding (the affine type system catches this naturally — capture-by-move *is* a move from the outer scope's perspective).

**Demo**:
```ts
const x = 42;            // Copy → captured by copy
const s = "hello";       // non-Copy → captured by move (s unusable after)
const f = () => console.log(x + ", " + s);
console.log(x);          // ok — i64 is Copy
// console.log(s);       // error: use after move into closure
f();
```

**Size**: ~250 LOC.

### P4.3 — Closure environment lowering

**Goal**: synthesize a `Type::Obj(StructId)` per closure expression. The struct is the captured-env. The closure value is a 2-tuple `(env_ptr, fn_ptr)`.

**Implementation**:
- Per closure expression, intern a struct layout from its capture set (ordered by variable name for stability).
- Closure value layout: 16 bytes — `{ env_ptr: i64, fn_ptr: i64 }`. SSA type: `Type::Closure(env_struct_id, fn_id)`.
- The lowered code pointer takes `(env_ptr: i64, args...) -> Ret`. Inside the body, captured-var reads become `Load(ty, env_ptr, field_offset)`.
- Calling a closure: `call_indirect fn_ptr, env_ptr, args...`.
- Closure drop: at scope exit of the binding, drop the env struct (recursive drop on each captured non-Copy field), then free the env (`__torajs_obj_drop`). Code pointer is in static text — no drop.

**Size**: ~400 LOC across check.rs (closure type inference) + ssa_lower.rs (env synthesis + call lowering) + both backends (call_indirect support).

### P4.4 — Returning closures + shared captures via Rc

**Goal**: the canonical "counter factory" example.

```ts
function counter(): () => number {
  const n = Rc.new(RefCell.new(0));
  return move () => {
    const m = n.clone();
    m.borrow_mut().value += 1;
    return m.borrow().value;
  };
}
const c = counter();
console.log(c());  // 1
console.log(c());  // 2
```

**Adds**:
- `move` keyword on closure expressions. Forces every capture to take place by-move (or by-clone for Rc — the closure's env owns its own Rc).
- Closures returned from functions: the env is heap-allocated (already via the P4.3 layout). The fn signature `() => number` becomes `Closure(<closure_id>)` — a unique closure type per expression. Storing it in a variable requires the closure type be nameable; for now, the inferred type stays anonymous (named via internal id), and direct-pass-around works. Trait-object closures (`dyn Fn() -> number`) wait for P14.
- `Rc<RefCell<T>>` (P2.3.e) for the shared mutable count state. `RefCell.new(0)` allocates an interior-mutable cell; `borrow_mut()` panics on aliased borrows.

**Size**: ~300 LOC (mostly in check.rs to track closures-as-return-types and explicit-move analysis).

### P4.5 — `FnMut` and mutation through `&mut self`

**Goal**: closures that mutate captured Copy state directly without `Rc<RefCell<...>>`.

```ts
function counter2(): FnMut() -> number {
  let n = 0;
  return () => { n += 1; return n; };
}
```

**Adds**: distinguish FnMut (mutating self.env) from Fn at typecheck. Receiver in the lowered fn becomes `&mut env_struct`. The closure value can only be invoked when held by `let` (mutable binding), not `const`.

**Risk**: this is where Rust's lifetime system would normally enter — `&mut self` borrow rules across multiple call sites. Without lifetimes, we punt: each FnMut call holds the receiver for the call's duration only, and closure values are not aliasable (affine — moving them into another binding consumes). The test case "store FnMut in two places" produces a compile error under P2.1 + P4.5 rules, which is the safe outcome.

**Size**: ~200 LOC.

### P4.6 — Bench cases for closures

Once P4.4 lands:
- `closure-counter-1m` — invoke a counter closure 1M times; baseline for FnMut overhead.
- `iterator-style-fold` — `[1..1M].map(...).filter(...).reduce(...)` once Array has the methods (post-P7.Array). Measures closure-per-element overhead.
- `callback-tree-walk` — recursive tree walk with a closure callback. Tests closure-as-arg call site overhead.

Targets: each within 1.3× of equivalent Rust closure code.

### P4 deliverables

- Closures with captures: by-move, by-copy, by-clone — all type-checked, all GC-free.
- `move` keyword forces by-move semantics.
- Returning closures from functions (heap env, dropped when closure value drops).
- Shared mutable state through `Rc<RefCell<T>>` capture pattern.
- FnMut closures with mutable env.
- ~1150 LOC across the 5 sub-phases. ~3-4 weeks of focused work.

### Open questions deferred past P4

- **Trait-object closures** (`(x: i64) => i64` as a type that erases the closure id) — needed for "store closures in a `Vec`" or "callback-style APIs that take any matching closure". Requires P14 trait machinery. For P4 we live with monomorphic-only.
- **Generic closures** (`<T>(x: T) => T`) — same gate.
- **`async` closures** — defer to P5.5 (after async lands).

---

## P5 — Async/await — state machine lowering + executor

The Rust shape: async fns compile to **state machine structs**, `await` is a yield point that saves all live locals into the state, and an executor polls until completion. No callback hell, no Promise chains, no microtask queue from JS engines — and critically, **no GC**. Futures are values with deterministic destruction; cancellation is a drop, not a third-party abstraction.

### Surface decisions

- **Type spelling**: `Promise<T>` for the surface (TS-shaped name), but the underlying type is Rust's `Future` shape. We expose **both** names: `Promise<T>` is an alias for the internal `Future<T>` type. Keeps source-level TS muscle memory, doesn't lock us into Promise-specific semantics (no `.then` / `.catch` / `.all` chaining — those are stdlib functions, not methods).
- **Top-level await**: allowed in the entry module only. Other modules (P9) cannot top-level await — they're lazily-evaluated and a top-level await would block the importer.
- **Async closures**: `async () => { ... }` syntax. Returns a closure whose call returns a `Future<T>`. Lowered like a regular closure (P4) where the body produces a state machine. Lands in P5.5.
- **`async` propagation**: async functions can only be awaited inside an async context. Sync fn calling an async fn requires manual blocking via `Future.block_on(f)` (a stdlib helper that drives a single future to completion on the current thread). No implicit blocking.

### State machine model (Rust-shape)

Each async fn `async fn foo(args) -> T` lowers to:

1. A **state struct** `FooFuture` holding: discriminant + all live locals at every await point + `args`. Layout is type-checked; size is the size of the largest live set across awaits.
2. A **poll fn** `Future::poll(self: &mut FooFuture, ctx: &mut Context) -> Poll<T>` that switches on the discriminant, executes the segment up to the next await, and either returns `Ready(T)` (if the body finishes) or `Pending` (if an inner await returned Pending). Updates `self.discriminant` to the next state on Pending.
3. A **constructor** `foo(args)` that returns `FooFuture { discriminant: 0, args, ...uninit_locals }` — does **no work**, just packs args. Async fns are lazy: nothing executes until the future is polled.

`Poll<T>` is `enum Poll<T> { Ready(T), Pending }`. `Future<T>` is a trait-equivalent `{ poll(&mut self, ctx: &mut Context) -> Poll<T> }`. Without trait objects (pre-P14) the `Future` shape is monomorphized per async fn — same model as Rust pre-`dyn Future`.

### Pinning — the load-bearing detail

Self-referential state machines (a local borrowed across an await) are sound only if the future doesn't move after polling once. Rust solves this with `Pin<&mut T>`. We have to solve it too — or design around it.

Two options:
- **Pin discipline (Rust path)**: introduce `Pin<&mut T>` as a type-system primitive. Higher cost; requires lifetimes for `Pin<&mut T>` references; clashes with our "no lifetimes pre-P14" stance.
- **Box-by-default (the cheaper path)**: every async fn invocation heap-allocates the `FooFuture` immediately (`Box<FooFuture>` shape) and the executor only ever holds `Box<dyn Future>` — i.e. boxed pointers, immovable by construction. Cost: one heap alloc per async fn call. Pays off because the alloc is outside the hot loop, and we get Pin safety without the type-system surface.

**Decision: box-by-default**. Async functions return `Box<FuturizedFn>` directly. Stack-allocated futures are a P12 perf optimization (escape analysis can stack-allocate the future when it doesn't outlive the calling stack frame).

This is also why we need P2.3 (Rc) before P5: the boxed future is owned by the executor, and combinators like `Future::join(a, b)` need to share polled state — `Rc<RefCell<FuturePair>>` style. P5 leans on the P2.3 runtime.

### P5.1 — Parser + AST + typecheck

**Goal**: `async function`, `async () =>`, `await expr` parse and typecheck.

**Adds**: keywords `async` / `await` (reserved words); AST nodes `AsyncFnDecl`, `Await`. Type checker: an `async fn` body's return type `T` becomes `Future<T>` at the call site. `await expr` requires `expr: Future<T>`, returns `T`. `await` outside an async context = error. `await` inside a non-async closure = error.

**Demo**:
```ts
async function delay(ms: number): Promise<void> { /* ... */ }
async function main(): Promise<void> {
  await delay(100);
  console.log("done");
}
```
typechecks; doesn't run yet (no executor).

**Size**: ~200 LOC.

### P5.2 — State machine lowering

**Goal**: async fns become state machine structs + poll fns at the SSA layer.

**Adds**:
- New IR pass `async_lower` runs after `check.rs`, before `ssa_lower`. Walks each async fn, identifies live-across-await locals, synthesizes the state struct.
- Per await: emit save-state code (write live locals to state struct fields), emit a poll dispatch to the inner future, branch on Ready/Pending, restore-state code on resume.
- `await` desugar:
  ```
  // source:                   let x = await fut;
  // lowered:                  loop {
  //                             match inner_future.poll(ctx) {
  //                               Ready(v) => { x = v; break; }
  //                               Pending => { self.discriminant = N; return Pending; }
  //                             }
  //                           }
  // resume entry at state N:  goto continuation
  ```
- All live locals at the await point are stored into the outer state struct fields, then restored on re-entry. Local layout planning is a small register-allocation problem — for v1 we conservatively store all locals (slot per live var); P12 can compress.

**Size**: ~600 LOC. The bulk of P5.

### P5.3 — Single-threaded executor

**Goal**: `tr run` of an async program drives the top-level future to completion.

**Adds**:
- Stdlib runtime: `executor::block_on(f: Future<T>) -> T` — single-threaded. Polls `f` on the current thread, blocks (via `parking_lot` / `std::thread::park`) when Pending until a waker is signalled, polls again, repeats until Ready.
- `Context { waker: Waker }` — Rust-shape. `Waker` is a fn pointer + data ptr that the future can store and call to signal "I'm ready, poll me again".
- Built-in primitive futures: `sleep(ms)` (timer-driven via OS timerfd / dispatch_after / kqueue), `ready(v)` (always-Ready), `pending<T>()` (always-Pending — useful for tests).
- Top-level async main: if `main` is `async fn main()`, the entry runtime calls `block_on(main())`. Synchronous main keeps the existing path.

**Demo**:
```ts
async function main(): Promise<void> {
  console.log("start");
  await sleep(500);
  console.log("after 500ms");
}
```

Runs end-to-end via `tr run` (Cranelift JIT executor) and `tr build` (LLVM AOT executor — same runtime code, statically linked).

**Size**: ~400 LOC for the executor + 100 LOC per primitive future.

### P5.4 — Combinators: `join`, `select`, `Future.all`, `Future.race`

**Goal**: standard async combinators, in stdlib not built-in.

**Adds**:
- `Future.join(a, b): Future<[A, B]>` — polls both, completes when both ready.
- `Future.select(a, b): Future<Either<A, B>>` — completes on first ready.
- `Future.all(futures: Future<T>[]): Future<T[]>` — generic Promise.all equivalent. Gates on P14 generics.
- `Future.race(futures): Future<T>` — same gate.
- Each combinator is itself an async fn, lowering through the same state-machine pass. Recursive use of P5.2 — proves the lowering scales.

**Size**: ~250 LOC for the simple cases; full set after P14.

### P5.5 — Async closures

**Goal**: `async () => { ... }` returns a closure whose call returns a future.

**Adds**: closure expression with `async` modifier; the closure type becomes `() -> Future<T>` instead of `() -> T`; capture analysis identical to P4.2 — captures live in the closure env, the future is built fresh per call.

**Size**: ~150 LOC, mostly check.rs typing rules.

### P5.6 — Cancellation via drop

**Goal**: dropping a future cancels it, deterministically. No `AbortController` — drop is the cancellation primitive.

**Adds**: futures implement the existing P2 drop machinery. Dropping a `BoxedFuture` mid-poll runs the destructor, which:
1. Drops live locals at the current state.
2. Drops any inner pending futures (recursive — they cascade-cancel).
3. Frees the state struct.

This makes "I started a fetch and don't want it anymore" trivially correct. Different from JS Promises (uncancellable) — by-design.

**Size**: ~200 LOC + per-state-machine drop synthesis (built into P5.2 lowering, completed here).

### Multi-core async — P6

The full **work-stealing multi-threaded executor** (Tokio-shape) is P6, not P5. P5 ships with a single-threaded executor sufficient for `tr run` smoke tests, top-level await, and stdlib combinator design. The state-machine lowering and `Future` shape don't change between single-thread and multi-thread; only the executor implementation does. P6 swaps the executor in.

### P5 deliverables

- Real async/await with state-machine lowering, no GC, no callback hell.
- Boxed-future ownership; cancellation via drop.
- Single-threaded executor in stdlib.
- Async closures (P5.5).
- ~1900 LOC across 6 sub-phases. ~6-8 weeks.

### P5 bench cases (gated)

- `async-1m-tasks` — spawn 1M trivial async tasks via `Future.all`, measure overall throughput vs Tokio's equivalent.
- `await-tight-loop` — 100K await of `ready(v)` futures; measures state-machine resume overhead. Targets: within 2× of Rust async equivalent.
- `cancel-cascade` — build a tree of 10K nested futures, cancel root, measure full-tree drop cost.

### Open questions deferred past P5

- **`Stream<T>`** (async iteration) — defers to P14+ (needs generics + traits). Workaround until then: async fns returning explicit `Future<Vec<T>>` for finite collections.
- **`async fn` in trait** (Rust's bête noire) — gate on P14+.
- **Cooperative vs preemptive scheduling** — P5/P6 ship cooperative only. Preemptive (timeslicing async tasks) is unspecified; not a near-term goal.
- **Async drop** (drop with await inside) — deferred indefinitely. Drop is sync. If cleanup needs await, the user spawns a detached task explicitly.

---

## P6 — Multi-core, `Send`/`Sync`, structured concurrency

The point at which the runtime stops being single-threaded. Everything from P0–P5 runs on one OS thread; P6 introduces real parallelism + shared-memory concurrency, with the type system policing data-race freedom.

This is the second-largest design surface in the roadmap (after P3). We follow the Rust playbook closely — `Send` / `Sync` auto-traits, work-stealing executor, `Arc<T>` for cross-thread shared ownership, `Mutex<T>` / `RwLock<T>` / channel primitives. All of it is the **mechanical** version of Rust's design; we don't try to invent here. The risk is in implementation depth, not architectural novelty.

### Hard requirement #5 recap

"Multi-core — Send/Sync in the type system; multi-threaded executor." This phase delivers it. Below the type-system surface, this means:

1. The type checker computes `Send` and `Sync` as **auto-traits** propagated structurally.
2. Cross-thread APIs (`spawn`, channel send) constrain their type parameters by `Send`.
3. Stdlib ships `Arc<T>`, `Mutex<T>`, `RwLock<T>`, `mpsc::channel<T>`, `oneshot::channel<T>`.
4. The async executor goes from single-threaded (P5.3) to work-stealing across N OS threads.

### `Send` / `Sync` semantics

| trait | meaning |
|---|---|
| `Send` | safe to **transfer** ownership to another thread |
| `Sync` | safe to **share** a reference across threads (`Sync ↔ &T : Send`) |

Auto-trait derivation: a struct is `Send` iff every field is `Send`; same for `Sync`. **Negative implementations are explicit** — `Rc<T>` is `!Send + !Sync` because the refcount is non-atomic. `Arc<T>` is `Send + Sync` if `T: Send + Sync`. `RefCell<T>` is `!Sync` (interior mutability without locks).

The check pass:
- Auto-derived for every user struct, recursively.
- User cannot override directly (no manual `impl Send`); they can opt out by including a `!Send` field (e.g. wrapping in `PhantomNotSend` until we build a real surface for negative bounds, P14).

### P6.1 — `Send`/`Sync` in the type system

**Goal**: every type has computed `Send` / `Sync` flags. Cross-thread API call sites constrain by these.

**Adds**:
- `check.rs` extension: `TypeFlags { is_send: bool, is_sync: bool }` per type. Computed when a type is registered (struct decl) or used (Rc/Arc/RefCell instantiation). For aliases, propagated through `parse_type`.
- New primitive marker types `Type::PhantomNotSend`, `Type::PhantomNotSync` — zero-sized, drop-only, mark a struct as opting out.
- Cross-thread call sites (`spawn`, channel) take a generic param `T: Send` (P14 dependency). Until generics land, P6.1 ships with **monomorphic** spawn — `fn spawn(f: () -> T)` with explicit type args at the call site, type checker validates `Send`-ness manually.

**Size**: ~250 LOC. The auto-trait propagation is non-trivial because of recursive types (Rc<T> where T contains Rc<T>) — solved by fixed-point iteration on a graph of struct dependencies.

### P6.2 — `Arc<T>` — atomic refcount

**Goal**: `Rc<T>`'s thread-safe sibling. Same surface, atomic counts.

**Adds**:
- `Arc<T>` type: same `RcInner` layout as P2.3 (`{strong: u64, weak: u64, payload: T}`), but `strong` and `weak` are operated on with atomic instructions.
- Runtime intrinsics `__torajs_arc_alloc / _clone / _drop` — atomic versions of the Rc set. `clone`: `fetch_add(1, Relaxed)`. `drop`: `fetch_sub(1, Release); if was_one { fence(Acquire); free }` — Boost-style atomic-RC pattern.
- `Arc<T>: Send + Sync` iff `T: Send + Sync`. Cross-thread sharing routes through Arc.
- Both backends emit atomic ops: Inkwell uses `LLVMBuildAtomicRMW`; Cranelift uses `MachInst::AtomicRmw` (already exposed in `cranelift-codegen`).

**Test**: `Arc<i64>` cloned across two threads, both decrement and the count stays consistent. `valgrind --tool=helgrind` (or Apple's `tsan`) finds no data races.

**Size**: ~250 LOC.

### P6.3 — Thread spawn + join

**Goal**: `thread::spawn(closure)` returns a `JoinHandle<T>`. `.join()` blocks until the thread completes, returns `Result<T, JoinError>`.

**Adds**:
- `thread` stdlib module wrapping `pthread_create` (darwin / linux) / `CreateThread` (windows). Spawns an OS thread, runs the closure, stores its return value in a shared one-slot atomic cell, signals via condvar on completion.
- `JoinHandle<T>` is the receiver end of that cell; `.join()` waits + reads.
- Type rule: `spawn(f: F) -> JoinHandle<F::Output> where F: FnOnce() -> T + Send + 'static`. For this phase the `'static` bound is implicit (no lifetimes pre-P14) — closures can only capture owned values, no borrows allowed (capture analysis from P4.2 already prevents that).

**Demo**:
```ts
const h = thread.spawn(() => {
  let sum = 0;
  for (let i = 0; i < 1000000; i++) sum += i;
  return sum;
});
const result = h.join();  // blocks
console.log(result);
```

**Size**: ~300 LOC.

### P6.4 — `Mutex<T>` / `RwLock<T>` / atomics

**Goal**: shared-memory concurrency primitives.

**Adds**:
- `Mutex<T>` — wraps `T`; `.lock()` returns a guard with deref to `&T` / `&mut T` (drop releases). Implemented over `pthread_mutex_t` for portability.
- `RwLock<T>` — same shape, multiple readers / one writer. `parking_lot` semantics (we'll likely call into `parking_lot` from Rust runtime for correctness, ~100 KB binary cost, acceptable).
- `AtomicI64`, `AtomicBool`, `AtomicPtr<T>` — minimal atomic primitives for lock-free patterns. Methods: `load`, `store`, `compare_exchange`, `fetch_add` (numeric only).
- `Mutex<T>: Sync if T: Send` — the textbook rule.

**Size**: ~400 LOC + parking_lot interop.

### P6.5 — Channels: `mpsc` and `oneshot`

**Goal**: message-passing primitives. The default cross-thread communication pattern.

**Adds**:
- `mpsc::channel<T>(buffer_size: number)` returns `(Sender<T>, Receiver<T>)`. Multi-producer single-consumer, bounded. Backed by a ring buffer + 2 condvars (notfull, notempty).
- `oneshot::channel<T>()` returns `(Sender<T>, Receiver<T>)`. Single-shot. Sender's `.send` consumes; Receiver's `.recv` is awaitable (`Future<T>` — gated on P5).
- Senders and Receivers are Send. T must be Send.
- Async-ready: `Receiver::recv_async()` returns a Future that registers a waker when the channel is empty.

**Demo (multi-producer worker pool)**:
```ts
const [tx, rx] = mpsc.channel<i64>(100);
for (let i = 0; i < 4; i++) {
  const tx2 = tx.clone();
  thread.spawn(() => {
    for (let j = 0; j < 1000; j++) tx2.send(j).unwrap();
  });
}
let total = 0;
for (let v of rx) total += v;  // closes when all senders drop
```

**Size**: ~500 LOC.

### P6.6 — Multi-threaded work-stealing executor

**Goal**: P5's single-threaded executor is replaced by a work-stealing one. Async tasks run on a pool of N worker threads.

**Adds**:
- `executor::Pool::new(n: number)` — N worker threads, each owns a local Deque<Task>; idle workers steal from victims chosen at random (Tokio-shape).
- `executor::spawn(f: Future<T>) -> JoinHandle<T>` — pushes onto the current worker's deque, or a global injection queue if called from non-worker thread.
- Wakers: each task carries a Waker that, when invoked, re-queues the task. Must be Send + Sync (atomically refcounted).
- `block_on(f)` from P5.3 still works — it now drives a single future on the calling thread synchronously, with an option to consume the work-stealing pool for inner tasks.
- Cooperative scheduling — tasks yield only at await points. **No** preemption. Documented limitation.

**Size**: ~700 LOC. Testing requires Loom (concurrency-property-testing tool) — track in test infrastructure.

### Acceptance + bench

After P6 the language can credibly run server workloads:
- HTTP server bench (echo + JSON parse + response) — comparable to a Rust+Tokio "hello" server within 1.5×.
- 1M-task async stress: `Future.all([...range(1M).map(async () => 1)])` — within 2× of Tokio.
- Channel throughput: 1M messages through mpsc — within 2× of `crossbeam-channel`.

These cases land in `bench/cases/` only after P9 modules — they need `import` to be sane.

### P6 deliverables

- Send/Sync auto-traits + propagation + cross-thread API constraints.
- Arc<T>, Mutex<T>, RwLock<T>, atomics.
- Thread spawn/join.
- Channels (mpsc + oneshot).
- Work-stealing async executor.
- ~2400 LOC across 6 sub-phases. ~10-12 weeks. **Largest single phase in the roadmap.**

### Open questions

- **`!Send` / `!Sync` user surface** — until P14 we have no negative trait bounds, so the marker-field workaround stands. Document the rough edges.
- **Async cancellation across threads** — when the holder of a `JoinHandle<T>` drops it, do we abort the inner task? Decision: yes, drop = cancel cascade. Implementation = sending a cancellation signal to the worker; the worker's poll loop checks the signal at every state transition. Costs one branch per await — acceptable.
- **Loom-shaped property testing** — needed to validate the executor + atomic-RC patterns. Adds significant CI cost. Tracked separately from P6.

---

## P7 — Standard library

**Canonical design doc**: `docs/stdlib.md`. That file holds the architecture (hardcoded-globals model + module-system future), naming conventions, error-model decision (deferred), the recipe for adding a new method (~30 LOC each), and the complete inventory of what's shipped vs what's missing. Read it for the *how*; this section holds the *when*.

Stdlib is **layered across phases**, not a single block of work. Slice 1 shipped with P3 closeout (Math primitives + console.log + String.length). The next major block is `Array<T>` runtime — biggest gap, gates the largest set of bench cases, and depends on no preceding phase beyond P2.4.

### Status snapshot

| slice | members | status | gated on | reference |
|---|---|---|---|---|
| **slice 1**: numerics + I/O floor | `console.log`, `Math.{sqrt,abs,floor,ceil,log,exp,pow,min,max,PI,E}`, `print_f64`, `String.length` | ✓ shipped 2026-04-29 | none | commits `9a499de` + `036e5ed` |
| **slice 2**: Array runtime | `Array<T>` heap layout + `.push/.pop/.length`, indexed read/write | next, see P7.Array below | P2 (drop semantics) — done | — |
| **slice 3**: Number → string | `n.toString()`, `n.toFixed(d)`, `parseInt`, `parseFloat` | after slice 2 | needs string-builder fast path | — |
| **slice 4**: Math + Date | `Math.{random,sin,cos,tan,atan2,round,trunc}`, `Date.now()` | after slice 3 | needs PRNG seed (Xorshift / PCG) | — |
| **slice 5**: Array combinators | `.map`, `.filter`, `.reduce`, `.find`, `.some`, `.every` | after P4 (closures) | takes a closure arg | — |
| **slice 6**: String methods | `indexOf`, `slice`, `split`, `includes`, `startsWith`, `endsWith`, `toUpperCase`, `toLowerCase`, `repeat`, `charAt`, `replace`, `trim` | after P14 (some need Array<string> return) | partial: P14 | — |
| **slice 7**: Hashed collections | `Map<K, V>`, `Set<T>` | after P14 (generics) | hard P14 dep | — |
| **slice 8**: I/O — `process` + `fs` + `fetch` | `process.argv`, `process.exit`, `process.stdout.write`, `fs.readFile`, `fs.writeFile`, `fetch` | after P5 (async) for fetch; process/fs anytime after slice 5 | P5 for fetch | — |
| **slice 9**: stderr console | `console.{error,warn,debug}` | anytime; trivial | none | — |

### P7.Array — `Array<T>` runtime — the next big block

`Type::Array(elem)` exists in the type system but lowers nowhere. This sub-phase builds the runtime.

#### Layout

```
Array<T> on the stack:    [ ptr: *mut ArrayHeader<T> ]   (8 bytes, single pointer)

ArrayHeader<T> on heap (allocated by __torajs_arr_alloc):
  offset 0:   len: u64       // number of elements
  offset 8:   cap: u64       // allocated capacity
  offset 16:  data: T[cap]   // 8-byte aligned; elements packed
```

Same `Vec<T>`-style header that Rust uses. 16 bytes overhead per allocation; element layout matches the SSA `Type::T` of the element type.

For `T = Type::I64` / `Type::F64` / `Type::Bool`: 8 bytes per element (bool padded; P12 will bit-pack). For `T = Type::Str` / `Type::Obj` / `Type::Rc`: 8-byte pointer per slot, payload heap-allocated separately.

#### Sub-step decomposition

- **P7.Array.a — parser + typecheck + literal**: `T[]` and `Array<T>` parse as `Type::Array(Box<Type>)`; `[1, 2, 3]` typechecks against the contained element types (homogeneous; element type inferred from first element + checked vs rest). Already partially in P1.10. ~150 LOC.
- **P7.Array.b — runtime intrinsics + literal lowering**: `__torajs_arr_alloc(elem_size, init_cap)`, `__torajs_arr_drop(ptr, drop_elem_fn, len)`, `__torajs_arr_grow(ptr, new_cap)`. Array literal `[a, b, c]` lowers to alloc + N stores at `offset = 16 + i * elem_size`. ~250 LOC.
- **P7.Array.c — indexed access**: `arr[i]` reads → `Load(elem_ty, arr_ptr, 16 + i*elem_size)` with bounds check (`if i >= len { panic_oob() }`). Write `arr[i] = v` → bounds check + Store. Bounds-check elision in P12. ~150 LOC.
- **P7.Array.d — `.length` + `.push` + `.pop`**: read len at offset 0; push grows when len == cap (doubling strategy, copies via memcpy + frees old buffer + updates ptr); pop reads + decrements len. ~200 LOC.
- **P7.Array.e — recursive drop**: `__torajs_arr_drop` walks `len` elements, calls `drop_elem_fn(slot_ptr)` for each non-Copy element, then frees the buffer. Drop-fn synthesis at IR level — same machinery as `Rc<T>::drop_payload` callback (P2.3.c). ~150 LOC.
- **P7.Array.f — Array-of-string / Array-of-Rc / nested arrays**: validates the per-T layout. Drop walker correctly handles each. ~80 LOC of testing.

Total ~980 LOC for the bare Array runtime; another ~600 LOC across slices 5/6 for combinators (post-P4) and string-returning methods (post-P14).

#### Bench cases (Array)

Land alongside slice 2:
- `array-sum-1m` — `[1..1M].sum()` analog: build, push 1M times, sum. Targets within 1.5× of Rust `Vec<i64>`.
- `array-sort-100k` — naive sort 100k random i64 (post-P4 for closure callback). Targets within 2× of Rust unstable sort.
- `array-filter-map-reduce` (slice 5) — pipeline. Tests closure inlining + bounds-check elision interaction.

### Ordering rationale

The "by-soonness" prioritization:
1. **Array runtime first** — every nontrivial program wants arrays. Eight bench cases get unblocked.
2. **Number formatting next** — needed for printing computation results meaningfully (`Math.PI.toFixed(2)`).
3. **Array combinators** *after* closures (P4) — they're closure-takers; can't ship without P4.
4. **Map/Set** *after* generics (P14) — generic K, V parameters are the whole point. Hardcoded `Map<string, i64>` etc. would scale poorly.
5. **`fetch` / async I/O** *after* async (P5) — by definition. Sync `fs.readFile` can land earlier.
6. **String methods** spread across slices because some need P14 (`split` returns `Array<string>` is fine pre-generics; `Array<T>` of a parameterized T is the real gate).

### Module-system gradient

After P9, the hardcoded-globals model in `check.rs` is **replaced** with import-resolved stdlib. `Math.sqrt` becomes `import { sqrt } from "std/math";`. The intrinsics still exist; they're just registered through the module system instead of identifier-name matching. `docs/stdlib.md` covers this transition in its "Generic future" section.

### P7 deliverables (cumulative)

- After slice 2 (Array): every primitive bench case can be expressed in idiomatic torajs, no SSA escape hatch.
- After slice 5 (Array combinators): functional-style bench cases match TS/Rust idiom.
- After P9 module system: hardcoded-globals gone; stdlib imports work the same as user code; users can wrap stdlib with their own modules.

---

## P8 — Cranelift backend ~~(superseded)~~

**Folded into P3.6 by 2026-04-28 pivot.** Cranelift is now the JIT backend for `tr run`, landing in P3 not P8.

---

## P9 — Module system

The point at which torajs stops being a single-file language. Every nontrivial program is multi-file; every stdlib API is a module under `std/`. P9 also unlocks the **graduation point**: labs/0001-walking-skeleton becomes `crates/torajs-core/` + `crates/tr-cli/` because the module-system rework touches every entry point and we'd rather rewrite once.

### Surface decisions

- **ESM-shaped syntax**: `import { sqrt } from "std/math"`, `import * as M from "./util"`, `export function foo()`, `export type Bar = ...`. Drops CommonJS (`require`), AMD, UMD — none of these align with our static-typed AOT model.
- **No dynamic `import()`** in v1. Dynamic import implies bundling-and-shipping JS-style runtime resolution; we don't have that. Adds in P10+ if the playground needs it.
- **No `import.meta`** initially. May add `import.meta.url` later for stdlib internals; user-level use is a `process.argv[0]` substitute.
- **No re-export by default**: `export * from "..."` works syntactically but the type checker fully resolves it at compile time. No runtime reflection on exports.
- **`.tora.ts` extension** for files that opt into our dialect; plain `.ts` files would imply we're trying to be tsc-compatible (we're not). Compiler accepts `.ts` for ergonomics, but stdlib uses `.tora.ts` to mark dialect-only.

### Module identity

A module is identified by its **canonical file path** (resolved + normalized). Two imports of the same canonical path return the same module — single instantiation.

Module **boundary**: each module gets its own type-namespace (struct decls, type aliases) and its own value-namespace (functions, top-level lets). Cross-module access requires explicit `export` + `import`. No file-scope leakage.

### Path resolution

Three resolver layers, tried in order:

1. **Stdlib paths** — bare specifiers starting with `std/`: `std/math`, `std/io`, `std/array`. Resolved by a built-in resolver pointing at the bundled stdlib (P9.5 — see below).
2. **Relative paths** — start with `./` or `../`. Resolved against the importing file's directory; `.tora.ts` extension implied if missing.
3. **Bare third-party packages** — initially **unsupported**. Eventually resolved against a `tora_modules/` directory (npm-shaped, but no package.json — a `tora.toml` describing the package). P9 ships only stdlib + relative; package manager is post-P11.

No `node_modules` resolution ever (it's not our ecosystem). No URL imports (Deno-shaped); the implicit fetch + cache machinery would be a security/runtime nightmare for our threat model.

### P9.1 — Parser + AST

**Goal**: parse `import` / `export` declarations correctly.

**Adds**:
- Tokens: `import`, `export`, `from`, `as`, `*`, `default` (we may not implement default exports; flag if encountered).
- AST: `ImportDecl { specifiers: Vec<ImportSpec>, source: String }`, `ExportDecl { kind: Named(Vec<...>) | All }`, `Module { decls: Vec<ModuleDecl>, imports: Vec<...>, exports: Vec<...> }`.
- `ImportSpec` variants: named (`{ a, b as c }`), namespace (`* as M`), default (parse-only error: "default exports are not supported in torajs").

**Defers**: type-only imports (`import type { Foo }`) are parsed and treated identically to value imports for the type-checker; AOT codegen elides them when possible.

**Size**: ~250 LOC.

### P9.2 — Module graph builder

**Goal**: from the entry file, build the full module graph (DAG with cycle detection).

**Adds**:
- New top-level pipeline stage: `module_graph = ModuleGraph::build(entry_file)`. Walks imports recursively.
- Cycle detection: import cycles are an **error** (Rust-shape). Reasoning: cycles bite at type-check time (forward references through cycles are a mess) and at AOT-init time (which module's top-level let initializes first?). Refactoring out a cycle is a one-time cost; living with cycles is permanent friction.
- Each module typechecks **independently** in topological order. A module's type-export list is computed before any importer typechecks. This makes incremental compile (P9.4) tractable.

**Size**: ~300 LOC.

### P9.3 — Cross-module type checking

**Goal**: types defined in module A are usable in module B; affine semantics are correctly tracked across module boundaries.

**Adds**:
- `Module::exports: HashMap<String, Export>` where `Export = Value(Type) | TypeAlias(Type) | Struct(StructId)`.
- The struct-interning table (`Module.struct_layouts` from P2.4) becomes a **whole-program** table — `WorkspaceCtx.struct_layouts` — so two modules declaring `{ x: i64 }` get the same `StructId`, and field offsets line up across the linker boundary.
- Imported names enter the importer's scope via aliasing: `import { sqrt } from "std/math"` is equivalent to a local binding `sqrt: <type from math module>`.
- Affine state is **not** crossed: a non-Copy value moved into module B can only be re-exported if B itself moves it. No "shared module-global with side effects" pattern from JS — tla statics that own resources work, but their drop is end-of-program.

**Size**: ~400 LOC.

### P9.4 — Multi-file compilation + incremental cache

**Goal**: `tr build entry.ts -o entry` compiles every reachable module, links them into a single binary. Repeated compiles reuse cached LLVM IR.

**Adds**:
- Per-module compile artifacts: `parse_tree.bin` (postcard-encoded), `type_check.bin`, `ssa.bin`, `llvm_ir.ll`. Cached at `~/.cache/torajs/<workspace_hash>/<module_hash>/`.
- Cache invalidation: hash of (file content + every dependency's exported-types hash). Touching a module invalidates only its downstream consumers.
- Linking: AOT path emits one object per module via `LLVMTargetMachineEmitToFile`, then system `ld` produces the final binary. Cranelift JIT path keeps everything in one CodegenContext (no on-disk linking).

**Compile-time target** (rustc-debug class):
- 10 KLOC project, cold compile: ≤ 30s
- 10 KLOC project, warm compile (one file changed): ≤ 5s

**Size**: ~600 LOC (cache + serialization + linking glue).

### P9.5 — Stdlib graduation: hardcoded globals → real modules

**Goal**: the `Math.sqrt` / `console.log` / `String.length` infrastructure that lives as hardcoded patterns in `check.rs` and `ssa_lower.rs` (per `docs/stdlib.md`) gets **replaced** by importable modules under `std/`.

**Implementation**:
- Stdlib lives at `crates/torajs-core/std/` as `.tora.ts` files: `std/math.tora.ts`, `std/io.tora.ts`, `std/array.tora.ts`, etc.
- Stdlib functions whose body is a runtime intrinsic use a `@intrinsic("__torajs_math_sqrt")` decorator-shaped attribute (we'll spell it as a comment-pragma `// @intrinsic ...` to avoid implementing decorators). The check pass treats `@intrinsic` as a body-elided declaration; the SSA lowerer routes the call to the named runtime symbol instead of emitting a fn body.
- `Math` becomes a namespace import: `import * as Math from "std/math";` — preserving the `Math.sqrt(...)` source syntax.
- `console` is similar: `import { log, error, warn, debug } from "std/io"` — but we keep an auto-injected pseudo-import in the entry module so that `console.log("x")` still works without an explicit `import` line. (Trade-off: less pure, but `console` is so universal that requiring an import everywhere would be visual noise. Revisit.)

**Migration path**: P9.5 lands the new system in parallel with the hardcoded one (both work). Then a sweep deletes the hardcoded matches in `check.rs` and `ssa_lower.rs`. Fully remove ~3 weeks after P9.5 ships.

**Size**: ~500 LOC for the import+intrinsic-pragma machinery; the hardcoded code being deleted is mostly net-negative LOC.

### P9.6 — Workspace `.toml` (eventually)

**Goal**: project root discovery for multi-package workspaces. Like Cargo's `Cargo.toml`.

**Adds**: `tora.toml` with sections `[package]`, `[dependencies]`, `[bin]`, `[lib]`. Resolver looks up the file from the entry, walks parents to find it, configures the workspace. Packages can depend on other packages by relative path or (eventually) registry name.

**Defers** to P11+ (after the package manager story exists). For P9 ship, single-file projects still work; multi-file projects use a default workspace at the entry's directory.

### P9 deliverables

- Multi-file compilation, ESM-shaped imports.
- Stdlib lives in real `.tora.ts` files; hardcoded globals deleted.
- Incremental compile within rustc-debug speed targets.
- Module-graph cycles rejected at parse-time.
- Workspace skeleton (`tora.toml`) lands but registry support is deferred.
- ~2000 LOC across 5+1 sub-phases. ~6 weeks.

### Graduation point — labs → crates

P9 closeout = move:
- `labs/0001-walking-skeleton/src/*.rs` → `crates/torajs-core/src/`
- `tr` CLI binary → `crates/tr-cli/src/main.rs`
- Stdlib → `crates/torajs-core/std/`
- `bench/` runner names update from `torajs` (still works) to `torajs` (no path change, just internal restructuring)

This is when the project lifts out of "research lab" status. Production-rules apply: tests, docs, CI gates. The `.claude/rules/rust/` standards activate on the now-`crates/`-resident code.

### Open questions

- **`std/` source distribution** — does the binary ship with stdlib `.tora.ts` source files (slow at compile time, simple at install) or pre-compiled module artifacts (fast but binary-coupled)? Likely both: stdlib source ships at install time, first compile pre-builds artifacts into the user cache. Future revisit.
- **Re-export hygiene** — `export * from "x"` across multiple chained re-exports could explode the symbol table. Cap depth or simply emit each export with provenance metadata.
- **Type-only imports** — `import type { Foo }` is parsed but treated identically. If we later want to elide import side-effects more aggressively, type-only is the lever.

---

## P10 — Playground on torajs.com

The public-facing surface for the project. Visitors paste/write torajs code, click Run, see output + every internal IR stage. No server-side execution — the engine itself runs in-browser as wasm. Sandboxed, free, no auth.

This phase is what hard requirement "first-class WASM target" is for: not "user code compiles to wasm" (that's a separate target, deferred — see open questions) but "**the torajs compiler+JIT runs as wasm in the browser**." The user pastes source, the wasm-loaded engine compiles + executes it via Cranelift JIT (post-P3.6, Cranelift has a wasm-backend for itself — we link it as a wasm32-wasi binary).

### Design contract

- **Browser-only**: nothing server-side. No deploy of user code; no persistence beyond `localStorage`. Simplifies abuse model and infrastructure.
- **Safe execution**: each Run gets a fresh wasm instance with a fixed memory budget (default 256 MB) and a hard CPU timeout (default 5s, configurable via URL param). Hitting limit cancels execution and reports.
- **First-class output panel**: stdout/stderr from the user's code, with ANSI color support (we'll likely strip ANSI for now and add later).
- **Internals tabs**: the same ones `tr` exposes via subcommand — Tokens / AST / IR / SSA / LLVM-IR / Cranelift-CLIF / objdump (only for AOT path; in-browser only does JIT). Educational.
- **Shareable URLs**: `?code=<base64-gzipped-source>` query param. Server-free; URL holds full state.

### P10.1 — Engine to wasm32-wasi

**Goal**: produce a `torajs-engine.wasm` that runs the entire compile + execute pipeline.

**Adds**:
- New build target in `crates/tr-cli/`: `cargo build --target wasm32-wasi --release --features wasm-host`. The `wasm-host` feature gates browser-specific runtime hooks.
- Cranelift's wasm backend: Cranelift can target wasm32 from itself (wasmtime uses this pattern — Cranelift compiled to wasm + then Cranelift JITs more wasm). We use the same. There's a known size cost (Cranelift codegen + LLVM crates compiled to wasm = large binary, target ≤ 5 MB gzipped; LLVM is ~80 MB, so we **drop Inkwell from the wasm build** — wasm build is JIT-only).
- WASI shim: `wasi_snapshot_preview1` polyfill in JS supplies `fd_write` (stdout), `clock_time_get`, etc. No filesystem; a virtual `/` with a single file holding the user's source. Implemented as a tiny JS shim (~200 LOC).
- Memory budget: wasm linear memory capped at 256 MB; allocator (`wee_alloc` or `dlmalloc`) returns fail when it can't grow.

**Demo**:
```html
<script type="module">
  const engine = await loadEngine();  // fetches torajs-engine.wasm + WASI shim
  const result = await engine.run("console.log('hello')");
  // result = { stdout: "hello\n", stderr: "", exit: 0, timing_ms: { compile: 4, run: 0.1 } }
</script>
```

**Size**: ~600 LOC (mostly JS shim + build config); ~3 MB gzipped wasm.

### P10.2 — Browser host JS package

**Goal**: a published-as-static-asset JS module `torajs-web` that wraps the wasm and exposes the API used by the playground (and any embedder).

**Adds**:
- `web/lib/torajs-web/index.ts`: exports `loadEngine()`, `Engine.run(source: string, opts?: RunOptions): Promise<RunResult>`, `Engine.compile(source): Promise<CompiledModule>` for separate compile/run cycles.
- Worker-isolation: each engine instance runs in a dedicated `Worker`. Main thread sends source via `postMessage`; worker compiles + runs + reports back. Hard CPU timeout enforced by main-thread `setTimeout(worker.terminate, t)`.
- Memory: each worker has its own wasm instance, so a runaway program doesn't leak into the next Run.
- Safe shutdown: terminating mid-execution leaks the wasm instance (worker is GC'd by browser); next Run spawns fresh.

**Size**: ~400 LOC.

### P10.3 — Web frontend (CodeMirror + tabs)

**Goal**: the actual `torajs.com/play` page.

**Adds**:
- React + React Router 7 page at `web/src/routes/play.tsx`.
- CodeMirror 6 editor configured for TypeScript syntax highlighting (no LSP yet — pure highlighting). Auto-saves to `localStorage` per-tab.
- Right side: output panel (stdout + stderr, colored by stream) + Run button + timing readout (compile_ms / run_ms).
- Tab strip: Output | Tokens | AST | SSA | CLIF | (settings).
- Each non-Output tab calls `engine.dump(stage)` instead of `engine.run(...)` — same wasm, different exit point. Cached per source-hash so flipping tabs is instant.

**Size**: ~800 LOC of React + 200 LOC of CodeMirror config.

### P10.4 — Shareable URLs + permalinks

**Goal**: every working example has a shareable URL.

**Adds**:
- `?code=<base64-gzipped>` query param. Page on load: if param present, source = decompressed, otherwise = `localStorage` last value, otherwise = a friendly default ("hello world" demo).
- Share button: copies the current URL with the param.
- URL length cap: ~8 KB (most browsers tolerate up to 32 KB but reasonable). Beyond that, show "source too long to share via URL" — fallback is gist-style integration (P10.5+).

**Size**: ~150 LOC.

### P10.5 — Curated examples

**Goal**: a left-side picker with ~15 curated examples — fib, mandelbrot, JSON shape, async fetch (post-P5), closures (post-P4), Rc-shared state (post-P2.3).

**Adds**:
- `web/src/data/examples/*.ts` — each export is `{ title, description, source }`.
- Picker UI populates from this directory; selecting one loads source + auto-runs.
- Each example is also a smoke test — CI runs `pnpm test:examples` which executes each via the wasm engine and compares output to a fixture. Catches regressions in the engine that break public-facing demos.

**Size**: ~200 LOC + per-example fixtures.

### P10 deliverables

- `torajs.com/play` ships, with Run button, internals tabs, share URLs, curated examples.
- Engine wasm + browser host package ship as static assets, deployed via the existing `web/` Caddy pipeline (per CLAUDE.md).
- ~2300 LOC across the 5 sub-phases. ~4-6 weeks.

### Open questions

- **User-code-to-wasm target** (different from "engine-as-wasm"): can torajs **emit** a wasm artifact for user code that runs outside the browser? Currently the AOT target is x86_64/aarch64 native via LLVM. Wasm is a 4th target, gated on Inkwell's wasm32 backend support + a wasm runtime layer (allocator + drops). Not P10's job — would be a separate phase.
- **Multiplayer / pair-programming** — visitors editing the same playground document. Out of scope for v1.
- **LSP in the playground** — browser-side LSP (TypeScript-style) would mean shipping the whole P11 LSP server compiled to wasm. Defer until P11 lands.
- **Persistent named playgrounds** — "save this snippet to /play/abc123". Needs a server. Eventually yes; for v1 the URL-encoded share is sufficient.

---

## P11 — Tooling — LSP / formatter / test runner / linter

The point at which torajs is **usable as a daily driver**, not just a research toy. None of these are research; they are mechanical translations of well-known designs (rust-analyzer, prettier, vitest, clippy) onto our type checker. Risk is in implementation depth, not architecture.

All four tools share a foundation: `crates/torajs-core/` (the type checker + IR + codegen) is the **library**, and each tool is a thin frontend over it. Same lesson as rust-analyzer/clippy/rustfmt sharing rustc internals — single source of truth for parsing/typing.

### P11.1 — LSP server

**Goal**: editor experience matching rust-analyzer's first 80% — diagnostics, hover, go-to-def, find-references, rename, document-symbols, completion (rough), workspace-wide error feed.

**Architecture**:
- `crates/tr-lsp/` — LSP frontend. Speaks JSON-RPC over stdin/stdout.
- Reuses `crates/torajs-core/` for parsing + type checking.
- **Incremental computation** via salsa (the rust-analyzer / rustc-driver pattern). Each query (e.g. "type of expr at position X") is memoized; file edits invalidate dependent queries only.
- Workspace model: shares the module graph from P9 — when a file edits, salsa recomputes the affected modules' type-check results, all downstream consumers re-validate against the new export types.

**Sub-steps**:
- **P11.1.a — Diagnostics + hover**: file-open / file-change → re-typecheck → publish diagnostics. Hover at position → return type from salsa query. ~600 LOC.
- **P11.1.b — Go-to-def + find-references**: maintain reverse-index `def_site → use_sites` from the typecheck pass. ~300 LOC.
- **P11.1.c — Completion (basic)**: identifier completion from in-scope names; member completion via type-driven lookup. **No** snippet generation in v1. ~400 LOC.
- **P11.1.d — Rename**: edit-distance-bounded multi-file rewrites. Validated by re-running typecheck on the proposed edit. ~300 LOC.
- **P11.1.e — Document symbols + workspace symbols**: outline tree per file; fuzzy search across the workspace. ~200 LOC.

**Editor integrations**: VS Code extension (in `tools/vscode-torajs/`) that wraps `tr-lsp` via the LSP protocol. Zed and Helix work via generic LSP config.

**Size**: ~1800 LOC across sub-steps. ~6-8 weeks.

### P11.2 — Formatter (`tr fmt`)

**Goal**: prettier-equivalent for our dialect. Format-on-save in editors.

**Architecture**:
- `crates/tr-fmt/` — uses `crates/torajs-core/` parser to get the AST + comments + trivia. Re-prints with canonical formatting.
- **Dprint-style** algorithm (rather than rustfmt's box-and-glue) — easier to implement, predictable output. Roughly: walk AST; per node, emit canonical layout; line-break only when necessary to fit within configured width.
- **Single-style policy**: no per-project config beyond line width. Prettier's "have an opinion, ship it" model. Prevents bikeshedding.
- **Trivia preservation**: comments attach to their associated node; reflow within constraints; never lose them.

**Sub-steps**:
- **P11.2.a — Pretty-print core grammar**: every AST node has a canonical print form. ~600 LOC.
- **P11.2.b — Comment + trivia attachment**: leading/trailing comments stay with their node. ~250 LOC.
- **P11.2.c — Line-break heuristics + width fitting**: fit in 100 cols by default, break at sensible points. ~300 LOC.

**Size**: ~1150 LOC. ~3 weeks.

### P11.3 — Test runner (`tr test`)

**Goal**: vitest-shaped test runner. `tr test foo.test.ts` executes test functions, reports pass/fail counts + diff on assertion failures.

**Surface**:
```ts
// foo.test.ts
import { test, expect } from "std/test";

test("addition works", () => {
  expect(1 + 1).toBe(2);
});

test("async sleep", async () => {
  const start = Date.now();
  await sleep(100);
  expect(Date.now() - start).toBeGreaterThan(99);
});
```

**Architecture**:
- `crates/tr-cli/` adds `test` subcommand. Discovers `*.test.ts` / `*.test.tora.ts` files. Per file: compile + run with a special `__test_main` entry point that walks registered tests + invokes each.
- `std/test` stdlib module: `test(name, fn)` registers the test in a per-module table; `expect(value).toBe(...)` returns a fluent assertion object that throws (or, post-P15, returns `Result<(), AssertionError>`) on mismatch.
- **JIT execution** via Cranelift backend by default — fastest dev loop. AOT mode for release-equivalent test runs.
- **Process isolation**: each test file runs in a fresh JIT instance; tests within a file share state (matches vitest default). Parallel: across files via thread pool (pre-P6 it's serial; post-P6 work-stealing).
- **Snapshot tests + fixture comparison**: `expect(x).toMatchSnapshot()` writes a `.snap` file on first run, diffs on subsequent. Useful for stdout fixtures.

**Sub-steps**:
- **P11.3.a — `std/test` module + `tr test` discovery + serial execution**. ~500 LOC.
- **P11.3.b — Assertion library**: `toBe / toEqual / toThrow / toBeCloseTo / toContain / toMatch`. ~300 LOC.
- **P11.3.c — Parallel runner + report formatting**. Post-P6. ~250 LOC.
- **P11.3.d — Coverage**: source-map-driven branch/line coverage. Far. Defers to post-P17.

**Size**: ~1050 LOC. ~3-4 weeks (excluding coverage).

### P11.4 — Self-hosted linter (`tr lint`)

**Goal**: a clippy-equivalent. ESLint cannot work for us — established at decision-time, kept here:

> ESLint targets ECMAScript and assumes a JS-grammar AST; our dialect adds `Rc<T>` / `move` / `Send` / `Sync` / affine types and drops `var` / `==` / `null` / decorators / `eval` / sloppy mode. ESLint cannot represent any of that. We need a linter built on our own AST + type checker — likely sharing the `check.rs` infrastructure with the type checker, the way clippy shares rustc internals. *(Decision: 2026-04-26.)*

**Architecture**:
- `crates/tr-lint/` — frontend over `crates/torajs-core/`. Each lint rule is a struct implementing `Lint { fn check(&self, cx: &CheckContext) -> Vec<Diagnostic> }`.
- Rules categorized: `correctness` (likely-bug), `style` (preference), `complexity` (refactorable), `perf` (slower than alternative).
- Suppression: `// tr-lint-disable rule_name [reason]` on the offending line. Reason required for suppression in CI mode.
- Auto-fix: rules that mark themselves `auto_fixable` provide an `apply(&Diagnostic) -> SourceEdit`. `tr lint --fix` applies all fixable lints.

**Initial rule set (~20 lints)**:
- `no_unused_let`, `no_unused_const`, `no_unused_import` — dead code.
- `prefer_const_over_let` — single-assignment lets.
- `unnecessary_clone` — `.clone()` on a value that immediately gets moved.
- `unused_rc` — `Rc::new` whose value never `.clone()`s. Likely a `Box`-equivalent (i.e. plain heap obj) is meant.
- `redundant_move` — `move` keyword on a closure that has no captures.
- `int_cast_truncates` — implicit i64→i32 narrowing (when cross-target codegen lands).
- `non_snake_case` / `non_camel_case` — naming convention checks.

**Sub-steps**:
- **P11.4.a — Lint framework + 5 starter rules**. ~600 LOC.
- **P11.4.b — Auto-fix infrastructure + 3 fixable rules**. ~300 LOC.
- **P11.4.c — `tr-lint-disable` suppression syntax**. ~150 LOC.

**Size**: ~1050 LOC for the framework + initial rules. ~3 weeks; rules accrete after.

### P11 deliverables

- LSP server with diagnostics, hover, go-to-def, find-references, rename, completion, document-symbols.
- Formatter (`tr fmt`).
- Test runner (`tr test`) with `std/test` stdlib + assertion library.
- Self-hosted linter (`tr lint`) with ~20 rules + auto-fix.
- VS Code extension wrapping the LSP.
- ~5000 LOC across all four tools. ~12-16 weeks.

### Open questions

- **Debugger** — defers to P17 (source maps + DWARF). LSP can show "step into" actions there.
- **Refactoring code actions** — extract function, inline variable, etc. Beyond P11 scope; build on top of LSP rename machinery.
- **`tr-lint` performance** — running 20 rules on a 10k-LOC project must finish ≤ 2s on my M4 Pro. Salsa-based incremental query layer (shared with LSP) keeps it within budget.

---

## P12 — Performance work

Open-ended; profile-driven. Examples of likely wins:

- Inline caches for dynamic call sites
- Shape caches for object property access
- Escape analysis for stack allocation
- Type-directed monomorphization at compile time (this might already be in P3)
- Concurrent garbage-free string interning

---

## P13 — LLVM `--release` mode ~~(superseded)~~

**Folded into P3.5 by 2026-04-28 pivot.** LLVM via Inkwell is now the primary AOT backend, not an optional `--release` mode.

---

## P14 — Generics

Type parameters on functions and types. The single largest type-system feature still missing — and the gate for `Map<K, V>`, `Set<T>`, `Future.all`, `Vec<T>::map`, `Result<T, E>` (P15), trait-object closures, and a real iterator API.

We follow the **Rust monomorphization** model. Each call site of a generic fn produces a per-type-args specialized instantiation; the SSA IR is fully monomorphized before LLVM/Cranelift see it. Trade-off: binary size grows with type variety; runtime is identical to hand-written specializations. Match Rust's defaults — code-bloat shows up only in pathological cases (e.g. `Vec<T>::push` instantiated 50 times); LLVM dedups identical bodies across instantiations after IR-level inlining.

### Surface

```ts
function identity<T>(x: T): T { return x; }
function map<T, U>(arr: T[], f: (x: T) => U): U[] { /* ... */ }

type Pair<A, B> = { first: A, second: B };
type Result<T, E> = { ok: T } | { err: E };  // (post-P15)

class Vec<T> { /* ... */ }   // (assuming class lands; we may keep it as plain `type Vec<T>`)
```

Constraints (`T extends Ordered`, `K extends Hashable`) require **traits** — Rust-shape. We don't have a trait system yet. Two options:

1. **Hardcode well-known constraints**: `extends Number`, `extends string`, `extends { compare(other: T): number }` — special-cased in check.rs. Cheap, scales poorly.
2. **Lightweight traits**: a trait is a struct of fn pointers, monomorphized vtable per impl. Closer to Rust. Heavier to implement.

**Decision deferred to P14.2** — start with hardcoded primitive constraints (Number, string, plus a `Comparable` shape derived from `compare`), revisit traits when the second nontrivial pain point appears (`Hashable` for Map<K, V>).

### P14.1 — Type parameters on functions

**Goal**: `function identity<T>(x: T): T { return x }` parses, typechecks, and runs.

**Adds**:
- Parser: `<T, U, ...>` after function name; AST `FnDecl.type_params: Vec<TypeParam>`.
- check.rs: `Type::TypeVar(TypeVarId)`. Inferred at call site from argument types (Hindley-Milner unification, restricted scope). Explicit `identity<i64>(5)` syntax also supported (mostly for disambiguation).
- Monomorphization pass: between check + SSA lower, walks the call graph, instantiates per type-arg combination. Each instantiation gets a unique mangled name (`identity__i64`, `identity__string`).
- SSA lower works on monomorphized fn table — no generic anything in SSA.

**Constraints in v1**: only `Copy` and `non-Copy` distinction — that already exists for affine types. Members on a `T: { length: number }`-shape constrained type are checked structurally per-instantiation (fail at instantiation site if missing).

**Size**: ~700 LOC, mostly the monomorphization pass + Type::TypeVar machinery.

### P14.2 — Trait-equivalent bounds

**Goal**: `function max<T: Ord>(a: T, b: T): T` works for any T that implements an `Ord` interface. Required for `Map<K: Hashable, V>`.

**Adds**:
- Trait declarations: `trait Ord { compare(other: Self): number }`. Surface syntax TBD — may use TypeScript-style interfaces with extra rules: `interface Ord { compare(this, other: Self): number }`.
- Impl blocks: `impl Ord for i64 { compare(other: i64): number { /* ... */ } }`. Or, struct-method shape: `class Foo { compare(other: Foo): number { ... } }` auto-implements an inferred trait.
- Monomorphization extends: per type-arg, look up the impl table; instantiate the impl's method bodies into the calling context. Methods inline as if hand-written — same perf as the non-generic version.
- Built-in core traits: `Eq`, `Ord`, `Hashable`, `Clone`, `Debug` (the latter for `console.log` of arbitrary types).

**Sub-step note**: traits can be deferred until concretely needed. Map<K, V> is the forcing function. Until P14.2 lands, generic fns work without trait bounds (P14.1 is enough for `identity<T>`, `Future.all<T>`, etc).

**Size**: ~600 LOC.

### P14.3 — Generic types (structs and aliases)

**Goal**: `type Pair<A, B> = { first: A, second: B }`; `type Maybe<T> = T | null` (post-P15); `class Vec<T>` (if classes land).

**Adds**:
- Type alias generics: `type Foo<T> = ...` substitutes T in the body at instantiation.
- Generic struct types: `type Pair<A, B> = { first: A, second: B }` interns one StructId per (A, B) instantiation. The struct-layouts table from P2.4 keys on the **fully concrete** types — `Pair<i64, string>` and `Pair<string, i64>` are distinct.
- Recursive generic types (linked list): `type List<T> = { head: T, tail: Rc<List<T>> | null }`. Requires breaking the recursion through Rc<T> — sound by P2.3.

**Size**: ~400 LOC.

### P14.4 — Generic closures + trait objects

**Goal**: closures parameterized by their input/output type; `dyn Fn` for storing closures of-some-shape in a Vec.

**Adds**:
- Generic closure expressions: `<T>(x: T) => x` — typechecks; instantiated per call type.
- Trait-object closure: `(x: i64) => i64` (without a generic param) is a concrete closure-type. We add `dyn Fn(i64) -> i64` as a virtual-dispatch closure type, fat-pointer (env_ptr + fn_ptr), where the fn_ptr part is dynamic. Required to store a heterogeneous list of closures (callbacks, event handlers).

**Size**: ~350 LOC.

### P14.5 — Variance + lifetime story (deferred decisions)

**Goal**: get the big design choices right before they bite.

**Decisions to make**:
- **Variance** of generic type parameters: `Vec<Cat>` should not auto-coerce to `Vec<Animal>` — invariance, by default. Same as Rust. Documented; no code work.
- **Lifetime parameters**: torajs has no lifetimes pre-P14. We've avoided needing them by routing borrows through `Rc<T>`. P14.5 confirms: lifetimes **stay deferred** — through P17. The cost is some performance left on the table (Rc clone is more expensive than a borrow); ergonomics offset is enormous (no lifetime annotations to teach).
- **Variance escape hatch**: `Cell<T>` is invariant; `Box<T>` is covariant; in Rust this is auto-derived. We follow auto-derivation rules.

**Size**: ~100 LOC of doc + check.rs hooks.

### P14 deliverables

- Generic functions, generic structs, generic type aliases.
- Trait-equivalent bounds (interface-shape).
- Trait-object closures (`dyn Fn`).
- Monomorphization at the SSA boundary — runtime perf identical to hand-written.
- ~2150 LOC across 5 sub-phases. ~6-8 weeks. **Largest type-system phase in the roadmap.**

### Open questions

- **Higher-kinded types** (`F<T>` where F is a type constructor) — almost certainly never. Adds enormous complexity; rare benefit.
- **Const generics** (`Array<T, N>` with N a number) — eventually useful for `[T; N]`-style fixed arrays. Defers to P14+1 if needed.
- **Specialization** (override a generic impl with a concrete one) — Rust's longest-running unstable feature. We won't ship it.
- **Trait inheritance** (`trait Sub: Super`) — probably yes when traits land, mirroring Rust's exact semantics.

---

## P15 — Error model

The biggest deferred design decision. Our stdlib doesn't ship error-returning APIs (`parseInt`, `fs.readFile`) until P15 lands, because every shipped API binds the language to one error model.

### The decision

**Result/Option (Rust-shape)**, not throw/catch. Recorded across multiple research logs and reaffirmed by `docs/stdlib.md`'s error-model section. Rationale:

1. **No tracing GC contract** — exceptions imply unwinding metadata, panic runtime, two-color basic-block CFG (normal + unwind paths). Adds non-trivial weight to the runtime and codegen.
2. **Type system enforces handling** — `Result<T, E>` is structurally distinct from `T`; you can't accidentally drop an error.
3. **Async + Result interact naturally** — `Future<Result<T, E>>` is just a Result-yielding future; no separate exception path through the executor.
4. **Easier to embed** — host code (P16) gets predictable error returns; no need to bridge a panic runtime to Rust's `Result` or to host languages.

We give up:
- Stack-trace ergonomics (Result errors don't carry stack frames automatically; we add an opt-in `stack_trace()` constructor that captures at error-creation time, optional cost).
- Familiarity for TS users (try/catch is muscle memory). Mitigated by clean propagation syntax (the `?` operator).

### Surface

```ts
type Result<T, E> = { ok: T } | { err: E };
type Option<T>    = { some: T } | { none: null };

function parseInt(s: string): Result<i64, ParseError> { ... }

function parseAndAdd(a: string, b: string): Result<i64, ParseError> {
  const x = parseInt(a)?;       // ? unwraps ok or returns err
  const y = parseInt(b)?;
  return { ok: x + y };
}
```

Decisions:
- **`?` operator** for early-return on err. Same as Rust.
- **`match` for explicit handling**. Same as Rust patterns. Lands as part of P15.
- **No `panic` for recoverable errors**. `panic!()` exists for "this program is unsalvageable" — divide by zero, array out of bounds, RefCell aliased borrow_mut, deliberate `unreachable!()`. Aborts the process; in a future P15+1 we may allow process-level catch via a `recover` handler.
- **`Result<T, E>` is layout-controlled**: tagged union with a 1-byte discriminant + max(sizeof(T), sizeof(E)) payload. Same as Rust enum. Single-pointer optimization for `Option<Rc<T>>` etc — null pointer = none.

### P15.1 — Result + Option types in stdlib

**Goal**: `std/result` and `std/option` modules; types are well-defined and usable manually.

**Adds**:
- Built-in tagged-union type kind: `Type::Enum(Vec<(VariantName, Type)>)` distinct from struct types.
- `Result<T, E>` and `Option<T>` as the first two enum types (built-in but expressible in user code once enum syntax lands).
- Constructors: `{ ok: x }`, `{ err: e }`, `{ some: x }`, `{ none: null }`. Pattern-matchable.
- **Open**: do we ship a general `enum` syntax in P15.1 or just Result/Option? Lean toward general enums — same machinery, more value.

**Size**: ~400 LOC for the enum type kind + Result/Option intrinsics.

### P15.2 — `match` expression

**Goal**: pattern matching across enums + literal patterns.

**Adds**:
- Parser + AST for `match expr { case_1 => e_1, case_2 => e_2, ... }`.
- Patterns: literal (`5`, `"hello"`), wildcard (`_`), variant binding (`{ ok: x }` binds `x`), guards (`if x > 0`).
- Exhaustiveness check: every variant of the matched enum must have a case (or `_` catch-all). Rust-style; checks at typecheck time.
- Lowering: chain of branches in SSA, one per case.

**Size**: ~500 LOC.

### P15.3 — `?` propagation operator

**Goal**: ergonomic `Result<T, E>` early-return.

**Adds**:
- Postfix `?` parses as `try_unwrap` op. Typechecker: requires receiver to be `Result<T, E>` or `Option<T>`; produces `T`; propagates the err/none variant by inserting an early-return. Receiver's outer fn must return `Result<_, E>` (or `Option<_>`) — error if mismatched.
- Lowering: `expr?` becomes `match expr { ok: x => x, err: e => return { err: e } }`. Inlined at IR level.
- Cross-error-type via `From`-trait coercion (post-P14.2 trait support): if outer fn returns `Result<_, E>` and `?` operates on `Result<_, F>` where `F: Into<E>`, we coerce. Optional in v1.

**Size**: ~200 LOC.

### P15.4 — Panic discipline + runtime

**Goal**: `panic!()` aborts cleanly with a useful diagnostic.

**Adds**:
- `panic!(msg)` intrinsic — calls `__torajs_panic(msg, file, line, col)` which prints to stderr and calls `abort()`. No unwinding; process is gone.
- Built-in panic sites: array out-of-bounds, integer divide-by-zero, RefCell borrow conflicts, `unreachable!()`.
- **Async context**: panic in an async fn aborts the executor and the process — same as a sync panic. We don't try to isolate panics per task (Rust does optional panic-isolation per task; we don't, simpler).
- **Optional `panic = abort` mode**: smaller binary, no unwinder linked. With our no-tracing-GC + no-exceptions design, we never unwind, so abort-only is the correct default.

**Size**: ~250 LOC.

### P15 deliverables

- `Result<T, E>` and `Option<T>` first-class.
- General `enum` syntax, `match` expression, `?` operator.
- Panic discipline. Process-level abort, no per-task isolation.
- Stdlib slices that previously held back error-returning APIs (`parseInt`, `fs`, `fetch`-error-cases) all unblock.
- ~1350 LOC across 4 sub-phases. ~4-5 weeks. **P15 typically lands alongside P14** since `Result<T, E>` is generic — P14.1 is a hard prereq.

### Open questions

- **Error context propagation** — `anyhow`-style chained errors. Probably ships as a stdlib helper post-P15 rather than a language feature.
- **Process-level recover** — long-running services want to log + restart on panic. Would add an opt-in `tora::set_panic_handler(fn)`. Probably yes, eventually.
- **Backtrace capture** — `Error.stack_trace()` — needs symbol info from P17 (DWARF). Until then, panic prints location only.

---

## P16 — Embedding API — `libtora` + `tora_eval`

The reason torajs **isn't a standalone runtime competing with Bun**: it's positioned as a **scripting-layer replacement for Lua in Rust hosts** (per CLAUDE.md). P16 is the API that makes that real — a C-ABI library + Rust wrapper that hosts call into to evaluate torajs source / call torajs functions / pass typed values across the boundary.

### Surface

```rust
// Rust host code
use tora::{Engine, Value};

let engine = Engine::new();
engine.eval(r#"
  function add(a: number, b: number): number {
    return a + b;
  }
"#)?;

let result: i64 = engine.call("add", &[Value::I64(2), Value::I64(3)])?.try_into()?;
assert_eq!(result, 5);
```

C-ABI shape:

```c
typedef struct tora_engine_s tora_engine_t;
tora_engine_t* tora_engine_new(void);
void tora_engine_free(tora_engine_t*);
int tora_eval(tora_engine_t*, const char* source, char** err_out);
int tora_call(tora_engine_t*, const char* fn, const tora_value_t* args, int argc, tora_value_t* out);
```

### P16.1 — `libtora.a` + `libtora.dylib`

**Goal**: build the engine as a static + dynamic library targetable from any host language. C ABI surface.

**Adds**:
- `crates/tora-ffi/` — wraps `crates/torajs-core/` with `extern "C"` functions. Uses `cbindgen` to auto-generate `tora.h` for C consumers.
- Dual targets: static (`libtora.a`) for embedders that want one binary; dynamic (`libtora.dylib`/`.so`) for Lua-style sidecar deployments.
- Memory management: `tora_engine_t*` is heap-allocated, owned by the host; host calls `tora_engine_free` on cleanup. All other `tora_*_t*` types follow the same own-pass-free protocol.

**Size**: ~600 LOC (mostly FFI boilerplate via cbindgen).

### P16.2 — Value bridge (cross-language type safety)

**Goal**: pass i64/f64/bool/string/Array/Obj/Rc values across the FFI boundary safely.

**Adds**:
- `tora_value_t` — tagged union: `{ kind: u32, payload: union { i64, f64, bool, *str, *arr, *obj, *rc } }`. Layout-controlled; can be used directly from C.
- For non-Copy types (string/Array/Obj/Rc), the host receives a **handle** (opaque pointer); operations on the handle go through `tora_*` accessor functions. Prevents host code from accidentally dropping torajs heap pointers.
- Reverse: torajs code can call host fns registered via `tora_register_host_fn(name, fn_ptr)`. Host fn signature: `fn(args: *const tora_value_t, argc: i32, out: *mut tora_value_t) -> i32`. Host fns are not generic; one-shape-per-registration.

**Size**: ~500 LOC.

### P16.3 — Rust idiomatic wrapper

**Goal**: thin Rust wrapper crate `crates/tora/` over `tora-ffi/` so Rust hosts have a clean API.

**Adds**:
- `Engine`, `Value` enum (Rust-side, distinct from FFI tagged union), `try_into` / `try_from` impls between `Value` and primitive Rust types.
- Trait `IntoValue` / `FromValue` so `engine.call("foo", &[1, "x", 3.14])` works without manual `Value::I64(...)` wrapping.
- `#[host_fn]` macro (procedural) to register a Rust fn as a torajs host fn, auto-generating the FFI shim.

**Size**: ~400 LOC + the proc-macro (~250 LOC additional).

### P16.4 — Sandboxing knobs

**Goal**: hosts that embed torajs as user-script-runner need limits — can't allow `fs` writes from a customer-uploaded script, or unbounded CPU/memory.

**Adds**:
- `EngineConfig`: max memory bytes, max CPU ms, max heap object count, allowed-stdlib subset (`["math", "string", "array"]` — no `fs`, `process`, `fetch`).
- Memory cap enforced by allocator wrapper (returns OOM after cap; torajs code panics deterministically).
- CPU cap enforced by interrupt-poll: every N basic blocks, codegen emits `if (deadline_ticks-- == 0) { tora_check_deadline() }`. Roughly 1% overhead.
- Stdlib subset enforced at compile time: imports that aren't in the allowed list = compile error.

This makes torajs **safe to embed in production hosts** — Lua's traditional pitch.

**Size**: ~700 LOC.

### P16 deliverables

- `libtora.{a,dylib,so}` + `tora.h`.
- Rust wrapper crate with `#[host_fn]` proc macro.
- Sandboxing config: memory / CPU / stdlib-subset limits enforceable.
- ~2200 LOC + ~250 LOC proc macro. ~6 weeks.

### Open questions

- **Sync vs async embedded engines** — for v1, `engine.call(fn, args)` is sync. Async hosts (Tokio runtimes) may want `engine.call_async()` returning a Rust `Future` that polls torajs's executor. Defers to P16+1.
- **Engine cloning / forking** — `engine.fork()` to give two threads independent stdlib state but shared compiled modules. Beyond v1.
- **Cross-language debugger** — host-side debugging of torajs code via lldb. Defers to P17.

---

## P17 — Source maps + debugging

The last piece for a daily-driver experience. Without P17, errors and panics report opaque "line N col M in compiled IR" diagnostics; with it, they map to the original `.tora.ts` source. Same for stack traces and step-debugging.

### Surface goals

After P17, the experience matches what a Rust developer takes for granted:

```bash
$ tr build foo.tora.ts -o foo
$ ./foo
thread 'main' panicked at foo.tora.ts:42:13:
   index out of bounds: arr.length=10, accessed=10
note: run with `RUST_BACKTRACE=1` (set TORA_BACKTRACE=1) for a full backtrace
```

```bash
$ tr debug foo.tora.ts
(tr-debug) break foo.tora.ts:42
(tr-debug) run
Breakpoint hit at foo.tora.ts:42
> let x = arr[i];
(tr-debug) print arr.length
arr.length = 10
(tr-debug) step
```

### P17.1 — Source position tracking through every IR pass

**Goal**: every SSA instruction carries a `Span { file: FileId, line: u32, col: u32 }` traceable to the original source.

**Adds**:
- Spans on every AST node (already partially in P0). Extend to every typecheck-introduced node (e.g. auto-inserted coercions inherit their parent's span).
- SSA `Inst { kind: InstKind, ty: Type, span: Span }`. Already roughly in place; tighten so every codegen-emitted inst has a non-default span.
- Codegen passes (Inkwell + Cranelift) attach span info at emit time:
  - LLVM: `MDNode` debug locations via `LLVMSetCurrentDebugLocation`.
  - Cranelift: `SourceLoc` per inst; threaded through `MachInst::source_loc`.

**Size**: ~400 LOC (mostly mechanical span propagation).

### P17.2 — Panic backtraces with source positions

**Goal**: `panic!()` and intrinsic panics print source-mapped frames.

**Adds**:
- AOT path: emit DWARF debug info via Inkwell's debug builder. Standard format; lldb / gdb ingest natively.
- Runtime: `__torajs_panic` calls libunwind to walk the stack, looks up symbols + line info via the embedded DWARF, prints a backtrace.
- JIT path: Cranelift backend registers an in-memory symbol resolver with the runtime; backtrace walker consults it instead of DWARF for JIT frames.

**Size**: ~500 LOC + libunwind dependency.

### P17.3 — DWARF emission — full debug info

**Goal**: `tr build --debug foo.ts` produces a binary that lldb can step through.

**Adds**:
- Variable info: every let binding registered in DWARF with name + type + storage location (stack slot or register at point of use).
- Type info: every torajs type lowered to a DWARF type DIE. Structs, Rc<T>, Array<T> all visible to debuggers.
- Inline info: monomorphized fn instantiations recorded with original generic source location.
- **Build-time gate**: full debug info enlarges binaries 5-10× and slows compile. Default build is no-debug (release-shape); `--debug` opts in.

**Size**: ~700 LOC. Mostly mechanical DWARF DIE construction.

### P17.4 — `tr debug` — interactive REPL + step debugger

**Goal**: a step-debugger built on Cranelift JIT + DWARF.

**Adds**:
- `tr debug foo.ts` enters a REPL. Commands: `break <file>:<line>`, `run`, `step`, `next`, `continue`, `print <expr>`, `bt`.
- Implementation: Cranelift JIT compiles with debug info; insert breakpoint as a trap inst at the requested PC. SIGTRAP handler enters REPL.
- Expression evaluator: compiles user-typed expressions in the current scope (vars in scope + types resolved from DWARF locals) via the same JIT pipeline.

**Size**: ~600 LOC.

### P17.5 — REPL (`tr repl`)

**Goal**: interactive REPL, separate from debug. Read-Eval-Print loop on torajs source.

**Adds**:
- `tr repl` enters an interactive shell. Each user line gets typechecked + JIT-compiled + executed in a long-lived module context. Top-level lets persist across lines.
- Line-buffered with rustyline (or a hand-rolled equivalent for Windows compat).
- Multi-line input handled by counting unclosed braces; auto-continue until balanced.

**Size**: ~400 LOC.

### P17 deliverables

- DWARF-grade debug info on AOT builds.
- Source-mapped panic backtraces (sync + async).
- `tr debug` step-debugger via Cranelift JIT.
- `tr repl` interactive loop.
- ~2600 LOC across 5 sub-phases. ~6-8 weeks.

### Open questions

- **Source maps for the playground** (P10) — JIT-only; no DWARF. Need a JS-side source-map format. Probably v3 source maps (npm-shipped, bun/node-compatible) emitted by the wasm engine. Defers to P10+P17 intersection.
- **Time-travel debugging** — record & replay the program, step backwards. Far. Probably never.
- **Async-aware step** — stepping through `await` should follow the resumption, not exit the function. Doable; integrates with state-machine lowering (P5.2). Lands as a refinement of P17.4.

---

## BENCH — cross-runtime perf benchmark (cross-cutting track)

A horizontal track running alongside P0 → P13, not numbered as a phase. Lives at `bench/` (top-level), implemented as a Rust harness crate that drives **bun, node, rust, go, python**, and torajs through a uniform per-case workload.

**Why it exists**: hard requirement #1 ("极致 perf — beat Bun/Node/etc on important benchmarks") is unmeasurable without a scoreboard. This track *is* the scoreboard. Every committed case must have tr passing — a permanently-failing tr would defeat the point. tr's language capability and the case set grow in lockstep.

**Status (2026-04-26): ✓ checkpoint A.** Workspace + harness skeleton + 5 runner descriptors + 1 case (`startup`) live; tr compiled into a Rust binary clears `console.log("x")` in ~1.3 ms, statistically tied with rust-native and ahead of bun/python/node on the same machine. First result snapshot committed at `bench/results/2026-04-26-mini-296b8aa.json`.

**Open follow-ups (deferred from RFC):**

- 4 more cases — `fib40`, `mandelbrot`, `json-parse-1mb`, `string-concat-1m`. Adding them is **gated by tr capability**: `fib40` needs P1.7 (functions), `mandelbrot` needs P1.1+P1.7+P1.6, etc. We will not commit a case tr can't pass.
- Peak RSS metric (`/usr/bin/time -l` parse) — deferred from checkpoint A.
- Binary-size metric for compiled targets — deferred.
- AOT-mode `compile_ms` for tr — populated only at P3+ when `tr build` produces a real artifact. Until then tr's `compile_ms` stays empty by design (the front-end work is microseconds, lost in process-startup noise — see RFC + `bench/cases/startup/README.md`).
- `/bench` Claude Code skill in dotclaude (out-of-repo). Wraps `cargo run -p bench-harness`.

RFC: `.claude/rfcs/20260426-bench-harness.md` (gitignored — populated locally via `devops dotclaude sync torajs`).

---

## Cross-cutting tracks

Work that runs **alongside** every numbered phase, not as one of them. Tracked here so it stays visible.

### Test infrastructure

- **Per-phase acceptance criteria** — each P-section above carries its own test plan. Cumulative test count drives a regression net.
- **Bench scoreboard as integration test** — every numbered case is an end-to-end test; a regression there is a P0.
- **Integration test crate** at `crates/torajs-itest/` (post-graduation) runs full `tr build` + execute on every example under `examples/`. CI gate.
- **Property testing** — quickcheck-style for the type checker's affine analysis (random ASTs, must reject use-after-move). Lands when affine bugs surface.
- **Fuzzing** — `cargo fuzz` targets for the lexer + parser (input: random bytes; assertion: never panic, only structured errors). Lands during the labs→crates graduation. ~1 person-week.
- **Loom-style concurrency testing** — for P6 multi-core. Schedules the executor against all interleavings of N atomic ops to catch ordering bugs. Adds significant CI cost.

### CI / release process

- **GitHub Actions on `develop`**: per-commit `cargo build` + `cargo test` + `cargo clippy --workspace --all-targets -- -D warnings` + `bun run check` for `web/`. Gates merge.
- **Release branches** per `git-flow` (per `.claude/rules/common/git-workflow.md`). `main` is production; `develop` is integration; phases close on `develop` and roll up to `main` at milestone tags (`v0.1` after P9 graduation, `v0.5` after P11, `v1.0` after P17).
- **Tag-driven artifact publishing**: on tag, build `tr` binary for darwin-aarch64 + linux-x86_64 + linux-aarch64 + windows-x86_64, package as a tarball, attach to GH release. Distributed via a future `tora-up` install script (`curl … | sh`). Naming caveat: `tr` collides with Unix translate-characters tool — rename happens before homebrew/packaging ship.

### Documentation

- **`docs/` is canonical** — this roadmap, `stdlib.md`, future `lang-reference.md`, future `embedding.md`. Versioned with the code.
- **Public website** at `torajs.com` — landing + playground (P10) + docs (later) + bench scoreboard (likely auto-generated from `bench/results/`).
- **No external blog/marketing** during research phase. Closed-source; communications happen on takagi's discretion.

### Performance work as a continuous track

P12 is the named bucket but perf work happens incrementally:
- After P3 closeout: codegen baseline established.
- During P4-P9: avoid regressing existing bench cases as features land.
- After P9 (graduation): formal perf RFCs land — escape analysis, bit-packing for bool, struct-of-arrays for hot loops.
- After P14 (generics): monomorphization-driven inlining tweaks.
- After P17: source maps unlock profiler workflows — perf work becomes profile-guided.

### Security / threat model (for embedding + playground)

- Threat model documented per surface area:
  - **CLI binary** — runs trusted user code; same threat model as Node/Bun. Stdlib `fs` / `process` access matches host privileges.
  - **Embedding API** (P16) — runs **partially-trusted** scripts (Lua-replacement). Sandboxing knobs in P16.4 mandatory; off-by-default = unsafe. Document.
  - **Playground** (P10) — runs **untrusted** code in an isolated wasm worker, hard memory + CPU caps. Fresh instance per Run.
- **No supply-chain story** until package manager exists. Stdlib + user-relative imports only — no third-party packages can introduce vulnerabilities.

---

## What this roadmap doesn't yet account for

Things not (yet) planned. Some have been promoted to phases since the roadmap's first draft; what remains is small.

- **Class syntax** — TS has classes; do we want them? They lower naturally to "struct + impl block" — same as Rust. Most likely **kept as syntactic sugar**: `class Foo { x: i64; bar() { ... } }` desugars to `type Foo = { x: i64 }` + `impl Foo { fn bar(self) { ... } }` (post-P14.2 traits). Decision pending until P14.2 lands; until then, struct + free fn pattern.
- **Conditional / mapped types** (TS exotic types: `Pick<T, K>`, `Partial<T>`, conditional `T extends U ? X : Y`). Probably never. We picked "TS surface we want"; these are TS-specific compiler tricks that bind to its inference model. Would massively complicate our type checker for marginal gain. **Not planned.**
- **Decorators** — TS's `@foo class Bar` decorators. **Not planned**; rejected at decision-time. The use cases (DI, ORM annotations) are better served by macros (P14.2+ or never) or by manual code.
- **JSX** — out of scope. Not React-like; not building a UI runtime.
- **Package manager** — eventually. Probably v0.5 (post-P11). `tora.toml` shape exists in P9.6; registry + publish flow lands later.
- **WebAssembly user-code target** (different from "engine-as-wasm" in P10) — emit wasm artifacts from user `.tora.ts` source for non-browser deployment. Beyond P14 timeline. Inkwell supports wasm32 backend; the runtime layer (allocator, drops) needs a wasm-shaped reimplementation.
- **`null` / `undefined`** — explicitly **dropped**. We use `Option<T>` (P15). Documented decision.
- **`==` / `!=`** — explicitly dropped (only `===` / `!==`). Per P1.5.
- **`var` / sloppy mode / `eval` / `Function` constructor** — dropped. Per `docs/stdlib.md` and the project decisions.
- **Cycle-collecting weak references** — `Weak<T>` slot is reserved in `Rc<T>` layout (P2.3); upgrade-to-strong API ships post-P15 with the Option type. No tracing GC ever; cycles in `Rc<T>` graphs leak by design.
- **`Symbol` / `Proxy` / `Reflect` / `Object.defineProperty` / `WeakMap` / `WeakRef`** — dropped per `docs/stdlib.md`. Static typing makes most unnecessary; refcount memory model makes the rest unsound.
- **Top-level non-async I/O**: handled. Sync `fs.readFile` ships in stdlib slice 8.

For the **promoted** items that are now phases:
- ~~**Error model**~~ → P15.
- ~~**Generics**~~ → P14.
- ~~**Source maps**~~ → P17.
- ~~**Incremental compilation**~~ → P9.4.

---

## Anti-patterns to avoid (lessons from other engine projects)

- **Don't ship an interpreter that doesn't share IR with the AOT path.** Two diverging implementations is a disaster.
- **Don't add features before the type system can express them.** Otherwise the type system gets retro-fitted poorly.
- **Don't optimize the interpreter.** It's for dev only. Optimization effort goes into the AOT backend.
- **Don't let `Rc<T>` leak beyond the Value module.** Other code shouldn't know whether Value is reference-counted or owned.

---

## Provenance

The decisions in this roadmap rest on the discussion logs in `.claude/researches/`:

- `0001-direction.md` — project framing, niche analysis (rejected "更先进的 bun" head-on; chose TS-native engine)
- `0002-engine-architecture.md` — perf layers, NaN-boxing, type-directed lowering as the structural moat
- `0003-no-gc-and-aot.md` — three positions A/B/C; chose C (Rust-shaped semantics + TS syntax)
- `0004-language-shape.md` — 7 hard requirements, working defaults for S1-S6
- `0005-roadmap.md` — original draft of this roadmap (now superseded by this file)

Those discussion logs are kept as audit trail. The canonical, living plan is this file.
