# 0004 — Language shape from the 7 requirements

**Status**: open discussion
**Started**: 2026-04-26
**Depends on**: `0001-direction.md`, `0003-no-gc-and-aot.md`
**Question**: takagi gave 7 hard requirements. Are they consistent? What language shape do they imply? Where do they need disambiguation?

## The 7 requirements

> takagi (2026-04-26):
> 1. 性能要非常非常好
> 2. 编译不能太慢
> 3. 可以解释执行
> 4. 没有 gc
> 5. 支持多核
> 6. 支持 async/await
> 7. 100% 兼容 ts6 语法特性

## Are they consistent?

Six are. One has a hidden interpretation choice that determines whether the whole set is consistent.

### The fork: req (7) — "100% TS6 syntax features"

Two readings of "兼容":

**(7-syntax)** Every TS6 program **parses** in torajs. The lexer/parser accepts everything `tsc` does. Programs that violate torajs's stricter semantic rules (e.g. ownership) fail at type-check time with a clear error.

**(7-behavior)** Every TS6 program **runs and behaves identically** to under `tsc`/Node/Bun.

(7-syntax) is consistent with (4) "no GC". (7-behavior) is **not** — TS as-spec'd is fundamentally a GC'd language (closures with mutable captures, freely shared mutable references, cyclic data, `eval`). You cannot run all TS programs faithfully without a GC.

This is the **same model Rust uses**: rustc accepts code that the borrow checker later rejects. Syntax accepted ≠ program valid.

### Resolution (2026-04-26)

> takagi: 我们先完成自己的，以后再去做 tsc bun 兼容，那是兼容性专门处理的事情.

**Compat is a downstream engineering concern, not a design driver.** We design torajs as its own language; tsc/Bun compat is something we attempt later, in a dedicated layer (e.g. a "permissive parser mode", or a transpiler from foreign TS to torajs TS), not a constraint on the core design.

This is **looser than (7-syntax)**: we're not even committing to parse every TS6 form. Concrete consequences:

- We pick the TS surface we like and ignore the rest. Examples we'll likely **drop**: `enum`, `namespace`, decorators (the legacy ones), `///` triple-slash directives, `module` (non-ESM), `var`, `with`, `eval`, sloppy mode, `==` (only `===`), implicit `any`, `null` (only `undefined`), `Function.prototype.bind`, prototype mutation.
- We pick the semantics we like even where syntax matches TS. Examples: `1 + "a"` is a type error, not `"1a"`; `0 == ""` doesn't even parse (no `==`); `null` not in the language.
- The 7 requirements still hold. But "100% TS6 兼容" (req 7) is now **rephrased** as "**TS-shaped syntax** for the features we choose to support" — TS is our visual identity, not our compatibility target.

This simplifies the parser, the type system, and v0 enormously. We only parse what we choose to support.

> Open follow-up: at some point we'll need a doc enumerating "what TS surface torajs supports" — but **after** we have something running, not now.

## Mapping each requirement to a design choice

Assuming (7-syntax):

| Req | What it forces | Implementation |
| --- | --- | --- |
| 1. 极致 perf | AOT compilation; static type-directed lowering; type-aware codegen; close-to-the-metal value rep | Cranelift (or LLVM) backend; NaN-boxing only for dynamic values, bare types for typed values |
| 2. Fast compile | Cranelift, not LLVM, as default; minimal IR passes; incremental compilation | Cranelift for dev; LLVM as opt-in `--release` |
| 3. Interpretable | A bytecode VM (or tree-walker over IR) that runs the same checked program | Frontend produces shared IR; both interpreter and AOT consume IR |
| 4. No GC | Static ownership tracking (Rust-style); deterministic destructors (Drop); no cycle-collecting tracing GC | Borrow check OR affine types OR region inference at the type-check stage |
| 5. Multi-core | Send/Sync-style thread-safety in the type system; either OS threads + Send constraints, or share-nothing isolates | Rust-style: `Send + Sync` traits, Mutex/Channel/Arc primitives, AOT-checked at type level |
| 6. async/await | First-class language syntax; futures-as-state-machines (zero-cost, like Rust); pluggable executor in std | TS6 has `async`/`await` syntax already — adopt directly, lower to state machines |
| 7. TS6 syntax compat | Hand-written TS-spec parser; full syntactic surface (generics, conditional types, mapped types, JSX, decorators, etc.) | parser is ~10K LOC of careful hand-written code; well-trodden territory (swc/oxc/tsc reference) |

Six of seven are consistent with **a single language shape**: 

> **A statically-typed, AOT-compiled language with TS syntax and Rust-shaped semantics: no GC, ownership/borrow checking, deterministic destructors, async/await as state machines, Send/Sync for multi-core, with an interpreter mode for development.**

This is internally consistent. It's also a Big Project — designing a language. But the requirements force it.

## Sub-choices that need explicit answers

These are second-tier choices that the requirements don't pin down on their own.

### S1 — What counts as "no GC"?

| Definition | Includes | Excludes |
| --- | --- | --- |
| **Strict (manual)** | `malloc`/`free` only; no automatic management | Rust's drop, refcount, anything else |
| **Rust-strict** | static ownership + Drop only; no `Rc`/`Arc` | refcounting, tracing GC |
| **Common (no tracing GC)** | static ownership + Drop + refcounting (`Rc`/`Arc`) | tracing GC, mark-sweep, generational |
| **No-pause** | anything that doesn't stop-the-world | concurrent tracing? |

I lean **"no tracing GC"** (common reading). Refcounting is allowed as a fallback when ownership can't be statically inferred. This is what Swift / Objective-C do, and they ship at scale with no GC pauses. Cycles leak unless we add a cycle collector — acceptable in practice.

The strict Rust-only reading is more elegant but limits expressiveness for things like callback handlers and tree structures with parent pointers.

> takagi: which?

### S2 — Ownership model

| Model | What user sees | Difficulty |
| --- | --- | --- |
| Borrow check (Rust-style) | lifetime annotations, `&` and `&mut`, sometimes painful | very high to implement, very high cognitive load |
| Affine (move-only, no borrows) | move semantics, `Box`/`Rc`-like wrappers when sharing | medium implementation, medium cognitive load |
| Region inference | mostly invisible; values get a region scope | high implementation (research-level), low cognitive load |
| Refcount everywhere | invisible | low; but worst perf |

I lean **affine + explicit refcount when needed** (`Rc<T>` syntax in TS). Skips the lifetime-annotation tax of full Rust while still being deterministic. Region inference would be more elegant but is a research project on its own.

Concretely: `let x: number[] = [1,2,3]; let y = x; // x moved; using x is an error` is the affine default. `let x: Rc<number[]> = Rc.new([1,2,3]); let y = x.clone()` for shared.

### S3 — Async runtime

| Model | What user sees | Examples |
| --- | --- | --- |
| **No runtime in std** | user picks executor (tokio-style); std has only `Future` trait | Rust |
| **Built-in single-threaded executor** | `await` Just Works, single-thread | JS event loop |
| **Built-in multi-threaded executor** | `await` Just Works, work-stealing | Go (goroutines), .NET TPL |

Req (5) wants multi-core, req (6) wants async/await. Combining these naturally points at **built-in multi-threaded executor**. Lighter cognitive load for users than Rust's "pick your runtime".

I lean **built-in multi-threaded work-stealing executor in std**, with `Send + Sync` checked at compile time. Users say `async function fetch(url: string): Promise<Response>` and it Just Works on multiple cores.

### S4 — Compile speed target

"编译不能太慢" — compared to what?

| Reference | Cold compile, ~10kLOC | Why |
| --- | --- | --- |
| `tsc --noEmit` | ~5s | type check only, no codegen |
| `swc` / `oxc` | ~0.5s | strip types only, no real check |
| `bun build` | ~0.5s | bundle, no type check |
| `cargo build` (debug) | ~30s | full Rust borrow check + Cranelift codegen |
| `cargo build` (release) | ~3min | LLVM, full optimization |

I'd target **rustc-debug class** (5-30s for ~10kLOC) — the price of doing real type/ownership checks. This is significantly slower than `swc strip-types` but unavoidable; we're doing real semantic work.

If the user wanted "as fast as swc" then we'd have to drop ownership checking, which conflicts with no-GC. So:

> takagi: is rustc-debug class compile time acceptable, or are we aiming closer to swc?

### S5 — Cranelift vs custom backend

For wasm output (and eventually native):

| Backend | Output speed | Compile speed | LOC for us | Notes |
| --- | --- | --- | --- | --- |
| **Direct wasm bytes** | mediocre | fastest | ~500 (wasm encoder) | works for v0; locks out native |
| **Cranelift** | ~80% LLVM | medium | ~thin wrapper over `cranelift_module` | both wasm and native targets |
| **LLVM** | best | slowest | ~thick wrapper over `inkwell`/`llvm-sys` | both targets, top perf |

I lean **Cranelift from v1** (Direct wasm in v0 walking skeleton, swap to Cranelift before perf matters). LLVM as `--release` mode much later.

### S6 — Interpreter mode form factor

Req (3) "可以解释执行" — what's the interpreter for?

- **REPL** — interactive prompt, line-by-line
- **Test runner** — run typed scripts without AOT compile
- **Hot reload during dev** — edit-save-run cycle
- **Production** — slow but acceptable for some uses (e.g. browser-side without wasm AOT)

I lean **REPL + dev test runner** as the primary use case. The interpreter doesn't need to match AOT speed; it just needs to run the same checked programs.

## What v0 looks like under this shape

The walking skeleton from 0003 stays mostly the same, but with **interpreter as the v0 backend** (because (3) requires it and it's smaller than AOT). AOT to wasm comes in v1.

`labs/0001-walking-skeleton/`:

1. Lexer (TS subset) — ~150 LOC
2. Parser → tiny AST (literal, fn call, var decl) — ~200 LOC
3. Type checker (trivial — only `number`, `string`, no inference yet) — ~80 LOC
4. Lower to IR (just SSA-shape stack machine ops) — ~80 LOC
5. **Tree-walking interpreter over IR** with refcount values — ~200 LOC
6. CLI: `tr run hello.ts` — ~80 LOC

Total: ~800 LOC. Deliberately no AOT, no async, no multicore in v0 — those are layered in over many subsequent labs/iterations.

Critical v0 commitments that don't lock us out:
- IR is a **separate stage** from AST, so AOT backend can plug in later
- Value type is opaque (NaN-box internally for dynamic; bare types for known-typed locals)
- AST is arena-allocated, `NodeId`-indexed
- No closures yet (they need ownership analysis); only top-level fns
- No async yet (needs state-machine lowering); only sync

## What v0 explicitly defers (because they're each their own multi-month effort)

| Feature | Lands in roughly |
| --- | --- |
| AOT to wasm | v1 |
| Cranelift backend | v2 |
| Closures + ownership inference | v2 |
| async/await | v3 |
| Multi-core / Send-Sync | v3 |
| Generics | v2 |
| Conditional/mapped types (full TS surface) | v3+ |
| Module system | v2 |
| Standard library (Array, Map, Set methods) | layered across all stages |
| LLVM `--release` mode | v5+ |

This is the "year+" plan. Pure research, no deadline, lots of throwaway expected.

## Tensions to resolve before any code

1. **T1**: ~~Confirm reading of req 7.~~ **Resolved 2026-04-26**: compat is downstream, design our own language with TS-shaped syntax for the features we pick.

2-7 (S1-S6) — recommended **working defaults** unless takagi pushes back. None of them block v0 walking skeleton (which uses only literals, function calls, no mutable state, no closures). They become load-bearing later in the order listed:

| Choice | Working default | Becomes load-bearing at |
| --- | --- | --- |
| **S1 — "no GC" definition** | No tracing GC; `Rc<T>` allowed for shared ownership when static analysis can't prove single-owner | v1 (heap allocation introduced) |
| **S2 — ownership model** | Affine (move by default) + explicit `Rc<T>` for shared; defer full borrow checking | v2 (closures introduced) |
| **S3 — async runtime** | Built-in multi-threaded work-stealing executor in std, like Go/.NET — not pluggable | v3 (async introduced) |
| **S4 — compile-speed target** | rustc-debug class (5-30s/10kLOC). Real type+ownership check costs that. | doesn't block; just calibrates expectations |
| **S5 — backend** | Direct wasm emit in v0; Cranelift in v1+; LLVM as opt-in `--release` much later | v0 wasm work, v1 native |
| **S6 — interpreter scope** | REPL + dev test runner; not a production target | v0 (interpreter is the v0 backend) |

These defaults are coherent. If takagi disagrees with any, push back and we revise; if no pushback, they become decisions and the lab starts.

## Out of scope for this doc

- The actual grammar of torajs (TS-spec verbatim minus some stuff plus some stuff — separate RFC when we get there)
- The standard library design
- Module system semantics (path resolution, loader)
- Tooling (LSP, debugger, formatter, package manager)
- Deployment story for compiled artifacts
- The full type system (structural? nominal? gradual? — separate doc)

## Next step

T1 resolved. S1-S6 set to working defaults pending pushback. Ready to open `labs/0001-walking-skeleton/`.
