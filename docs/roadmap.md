# torajs roadmap

> Canonical implementation plan. Living document — update as work progresses, decisions change, or steps reveal new sub-steps.
>
> Provenance: synthesized from `.claude/researches/0001-direction.md` through `0005-roadmap.md` (research / discussion logs, kept for audit trail).
>
> Last revised: 2026-04-26

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
| Compiler backend (initial) | Direct wasm-bytes encoder in v0; Cranelift from v1+; LLVM as opt-in `--release` much later | 0002, 0004 |
| Concurrency | Built-in multi-threaded work-stealing executor in std; `Send`/`Sync` traits enforced statically | 0004 |
| Compat with `tsc`/Bun | Not a design driver. Compat layer is downstream future work. | 0004 |
| Test262 conformance | Not a goal | 0001 |
| First-class WASM target | Yes — torajs.com playground depends on it | 0001 |
| Project repository home | `crates/` (Rust workspace), `web/`, `labs/`, `examples/`, `docs/` | 0001 |

### Working mode

- Closed-source research project. Many experiments and 废案. Advance step by step.
- New ideas first land in `labs/`. Graduation to `crates/` when stable.
- No tests/CI/docs pressure on `labs/` code; production rules apply once code lives in `crates/`.
- Be willing to delete more than is kept.
- See `.claude/rules/common/` and `.claude/rules/{rust,typescript}/` for shared coding standards.

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

| Phase | Theme | Rough timeline | Step count |
| --- | --- | --- | --- |
| P0 | Walking skeleton — `tr run hello.ts` prints `hello` | days | 6 |
| P1 | Core language — arithmetic, vars, control flow, fns, strings, arrays | weeks | 10 |
| P2 | Ownership — affine + `Rc<T>` + Drop | weeks | 4 |
| P3 | AOT to wasm — `tr build` produces .wasm | weeks-month | 5 |
| P4 | Closures with ownership analysis | weeks | 3 |
| P5 | async/await | month+ | 4 |
| P6 | Multi-core, Send/Sync | month+ | 3 |
| P7 | Stdlib (Array/Map/Set, Result/Option, console, fs, fetch, ...) | layered | n |
| P8 | Cranelift backend (replaces direct wasm + adds native) | month | 3 |
| P9 | Module system, multi-file | weeks | 3 |
| P10 | Playground on torajs.com | weeks | 3 |
| P11 | LSP + tooling | month+ | 3 |
| P12 | Perf work — ICs, shape caches, profile-guided optimization | open-ended | n |
| P13 | LLVM `--release` mode | optional, far | 2 |

Total time-to-mature: 18-36 months. Multi-year. Acknowledged.

---

## P0 — Walking skeleton

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

The language has a real heap now (strings, arrays). Time to make ownership real.

### P2.1 — Affine type system: detect use-after-move

**Demo**: `tr check` of `let a = [1,2,3]; let b = a; console.log(a[0])` errors with "value moved into b at line 1, used here".

**Adds**: type checker tracks "moved" status per binding; assignment moves; using a moved value errors with span. Primitive types (`number`, `boolean`) are `Copy` — moved means duplicated, no error. Heap types (`string`, `T[]`, `Rc<T>`) are affine.

**Size**: ~250 LOC

### P2.2 — Drop semantics

**Demo**: `tr run --trace-drop` of a program shows when each binding is dropped.

**Adds**: end-of-scope drops in IR (`Drop(local_id)` op), interpreter calls value's destructor (refcount decrement for `Rc<T>`, free for owned). Verify with leak-check (e.g. count allocs/frees).

**Size**: ~200 LOC

### P2.3 — `Rc<T>` first-class

**Demo**: `tr run` of `const a = Rc.new([1,2,3]); const b = a.clone(); console.log(a[0]); console.log(b[0])` works (no use-after-move because `clone()` produces a new Rc).

**Adds**: `Rc` as a built-in generic type; parser handles `Rc<T>` syntax; `Rc.new(x)` and `.clone()` builtins; refcount semantics in runtime

**Size**: ~250 LOC

### P2.4 — Object literals and structural types

**Demo**: `tr run` of `type P = { x: number, y: number }; const p: P = { x: 1, y: 2 }; console.log(p.x + p.y)` prints `3`.

**Adds**: object literal AST/parser, `type` aliases, structural type checking, member access lowering to offset (struct-style — typed fast path), affine ownership for objects

**Size**: ~300 LOC

### P2 deliverables

- Real ownership semantics; no GC; programs run with deterministic destruction
- The language can express enough to be self-hosted (in principle)
- ~4000 LOC total

**Graduation point**: at end of P2, `labs/0001-walking-skeleton/` graduates to `crates/torajs/` (or `crates/tora-engine/`, name TBD per 0001 naming concern). Lab gets archived/deleted.

---

## P3 — AOT to wasm

The interpreter still works for dev, but now we add a parallel backend that emits wasm.

### P3.1 — Wasm encoder, stub module

**Demo**: `tr build hello.ts -o hello.wasm` produces a wasm binary that, when run via `wasmtime hello.wasm` (or the bundled `wasmi` host), prints `hello`.

**Adds**: `wasm-encoder` crate dependency; wasm module with imports `print`, exported `_start`; emit a wasm function that calls `print` with hardcoded "hello" pointer/length

**Size**: ~250 LOC

### P3.2 — Number arithmetic in wasm

**Demo**: `tr build` of `console.log(1 + 2)` emits wasm that prints `3`.

**Adds**: lower IR arithmetic ops to wasm `i64.add` / `f64.add` / etc. based on type info attached to IR

**Size**: ~150 LOC

### P3.3 — Functions and locals in wasm

**Adds**: each torajs function becomes a wasm function; wasm locals; wasm calling convention

**Size**: ~250 LOC

### P3.4 — Strings in wasm linear memory

**Adds**: data section for string constants; runtime `print(ptr, len)` host function; pointer + length string representation

**Size**: ~250 LOC

### P3.5 — Heap allocator in wasm

**Adds**: bump allocator in wasm linear memory; `Rc<T>` layout (refcount + payload); drop emits decref + free

**Size**: ~400 LOC

### P3 deliverables

- `tr build` produces standalone .wasm files
- Basic programs (arithmetic, variables, functions, strings, arrays, Rc) all run via the wasm backend
- Two backends now coexist: interpreter (P0-P2) and AOT-wasm (P3)
- ~6000 LOC total

---

## P4 — Closures with ownership

The hard part. Most languages get closures wrong w.r.t. memory.

### P4.1 — Closures that don't capture

**Adds**: `() => { ... }` already works as arrow fn; nothing new.

### P4.2 — Closures with `move` semantics

**Demo**: `tr run` of `function counter() { let n = 0; return () => { n = n + 1; return n } } const c = counter(); console.log(c()); console.log(c())` prints `1` then `2`.

**Adds**: closure values that own their captures; analysis decides which variables are captured by move vs share; `Rc<T>` capture for shared mutable state. This is **the** hard step — it's where the no-GC design proves itself or doesn't. Plan a separate RFC.

**Size**: open. Multi-week.

### P4.3 — Closure ownership inference / `move` annotation

**Adds**: `move` keyword on closures (Rust-shaped); type checker infers default for simple cases

---

## P5 — Async/await

### P5.1 — `async` and `await` syntax

**Adds**: parser handles `async function` / `await expr`; type system has `Promise<T>` (or our equivalent — maybe rename to `Future<T>`?).

### P5.2 — Lower async to state machine

**Adds**: each async fn compiles to a state machine struct; `await` becomes a yield point; like Rust's async desugaring.

### P5.3 — Single-threaded executor in std

**Adds**: `tr run` invokes a built-in mini executor for the top-level future.

### P5.4 — Multi-threaded work-stealing executor

**Adds**: real executor like Tokio; work-stealing scheduler.

(Multi-threaded async overlaps with P6.)

---

## P6 — Multi-core / Send-Sync

### P6.1 — Send/Sync traits in type system

**Adds**: built-in marker types; type checker propagates Send/Sync; concurrent primitives `Mutex<T>`, `Arc<T>` (Rc's atomic sibling).

### P6.2 — Spawn threads / structured concurrency

**Demo**: `tr run` of a program that spawns workers and joins them.

### P6.3 — Channels / message passing

---

## P7 — Standard library

Layered across phases. Not a single block of work. Order roughly by how soon it's needed:

- `console.log/error/warn/debug` — P0 (host)
- `Math` (sin, cos, sqrt, etc.) — P1
- `Array` methods (push, pop, map, filter, reduce) — P1-P2
- `String` methods (indexOf, slice, split, replace) — P1-P2
- `Map<K,V>` / `Set<T>` — P2
- `Result<T,E>` / `Option<T>` — P2 (we may pick these over throwing exceptions)
- `Promise<T>` / `Future<T>` — P5
- `fs` (read, write, exists) — P5+ (CLI binary only, not wasm)
- `fetch` — P5+
- `URL` — P5+
- `process.argv`, `process.exit` — anytime
- Date/Time — P5+

---

## P8 — Cranelift backend

**Replaces** the direct wasm encoder with Cranelift. Adds native targets.

### P8.1 — Cranelift wasm backend
### P8.2 — Cranelift native (x86_64, aarch64) backend
### P8.3 — `tr build --target native` produces a native binary

---

## P9 — Module system

### P9.1 — ESM `import` / `export` syntax
### P9.2 — Path resolution (relative imports first)
### P9.3 — Multi-file compilation

---

## P10 — Playground on torajs.com

### P10.1 — Build the engine to wasm with browser host
The wasm build of torajs **as a JS-callable module**, where browser JS supplies `print` etc.

### P10.2 — Web frontend wires CodeMirror to the engine
Editor on the left, output on the right. Run button. Source updates → re-eval.

### P10.3 — Show internals tabs
Tokens / AST / IR / Wasm bytes — for the educational angle.

---

## P11 — Tooling / LSP

### P11.1 — LSP server for editor support (diagnostics, hover, go-to-def)
### P11.2 — Formatter
### P11.3 — Test runner (`tr test foo.test.ts`)

---

## P12 — Performance work

Open-ended; profile-driven. Examples of likely wins:

- Inline caches for dynamic call sites
- Shape caches for object property access
- Escape analysis for stack allocation
- Type-directed monomorphization at compile time (this might already be in P3)
- Concurrent garbage-free string interning

---

## P13 — LLVM `--release` mode

Optional. Cranelift may stay good enough.

---

## What this roadmap doesn't yet account for

Things not yet planned; surface as we hit them:

- **Error model** (exceptions vs Result-based) — design decision pending; tentatively favor Result/Option, no throw/catch
- **Class syntax** — TS has classes; do we want them? Maybe lower to "constructor fn + struct type". Or drop them entirely (research project — drop is plausible).
- **Generics** — must come before serious stdlib. Maybe P2 or early P3.
- **Conditional / mapped types** — TS's signature exotic types. May never implement; we picked "TS surface we want".
- **Decorators** — drop entirely?
- **JSX** — out of scope (we're not React-like)
- **Source maps** — eventually, for the playground debug experience
- **Incremental compilation** — for the rustc-debug-class compile speed target

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
