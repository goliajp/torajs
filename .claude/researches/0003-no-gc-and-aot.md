# 0003 — No-GC and AOT compilation

**Status**: open discussion
**Started**: 2026-04-26
**Depends on**: `0001-direction.md`, `0002-engine-architecture.md`
**Question**: 0002 implicitly assumed an interpreted/JIT'd, GC'd engine — the mainstream JS-engine shape. Should torajs instead be AOT-compiled and GC-free?

## The two questions are one

> takagi: 1. 为什么一定要 gc 呢，无 gc 语言不是更好吗
> takagi: 2. 是不是可以考虑做成编译型

These are correlated. Static lifetime tracking (the thing that makes a language no-GC) requires the compiler to see the whole program ahead of time. If you have AOT compilation **and** you commit to tracking ownership/lifetimes statically, you get no-GC for free. That's literally Rust's design.

So the real question is: **do we redesign torajs as a Rust-flavored language with TS syntax, or do we stay TS-shaped (with GC, dynamism) and get perf via JIT?**

These are two structurally different projects with different difficulty profiles, different end products, and different value propositions.

## What in TS actually forces GC?

If we want no-GC, we have to know what we're cutting. Concrete features in TS that, **as currently spec'd**, force a GC:

| Feature | Why GC | Can it be removed? | Cost of removal |
| --- | --- | --- | --- |
| **Closures capturing by reference** | Captured var outlives enclosing fn; no clear owner | Restrict to capture-by-value, or require explicit ownership annotations | Loses idiomatic JS callbacks; either annotate or use `move`-style closures |
| **Shared mutable references** (`const a = obj; const b = a; a.x = 1; b.x // 1`) | Two refs to same heap obj both alive; reaper unclear | Borrow checker (à la Rust): single mutable ref OR multiple immutable | Major language constraint; programs need rewriting |
| **Cyclic data** (linked lists with parent ptrs, graphs) | Refcount leaks them; tracing GC handles them | Use indices into arrays instead of pointers, OR weak refs, OR explicit arenas | Constraint-but-livable; common pattern in Rust |
| **Strings as growable, freely-shared values** | `"a" + "b"` allocates; nobody owns it | Distinguish `String` (owned) from `&str` (borrow); explicit lifetime | Forces TS programmers to think about ownership |
| **Arrays/Maps/Sets as freely shared mutable containers** | Same as strings | Same — owned vs borrowed views | Same |
| **Throw/catch with non-local exits** | Captured stack must release locals; objects in flight must survive | RAII via deterministic destructors | Workable; Rust uses Drop, no GC needed |
| **`eval`, dynamic `import()`, REPL** | Code dynamically introduced doesn't have known lifetimes | Drop these features OR have a small GC region for them | Drop them: can do AOT. Keep them: need a small dynamic heap. |
| **`any`** | Erased type; behaves as a JS value with shared heap semantics | Make `any` a runtime-tagged box with refcount; or ban `any` outright | Most TS code uses `any` rarely; banning is plausible for research |

The pattern: **TS-as-spec'd is a GC'd language**, but most idiomatic, well-typed TS doesn't actually need most of the GC machinery — it just allows it. If we're willing to add lifetime/ownership annotations to the type system, we can eliminate GC for the well-typed subset.

## Three coherent positions

These are three internally consistent designs torajs could take. Pick one; mixing them halfway gets the worst of all.

### Position A — Mainstream-shaped (what 0002 assumed)

- TS-as-spec'd; full compatibility with `tsc` semantics
- Interpreted → bytecode VM → JIT, like JSC/V8
- Generational tracing GC
- All of TS's dynamism preserved
- Perf path: catch up to JSC over years of engineering

**Pros**: programs that run on Node/Bun/Deno run on torajs. Familiar shape. Existing TS knowledge transfers directly. The "TS-native" angle still gives some perf wins (parse-time monomorphization for typed code) but doesn't change the category.

**Cons**: we're racing 30 engineers + $35M of capital on Bun + 200 engineers on V8. The structural moat is small.

### Position B — Hybrid (AOT for typed, GC for dynamic)

- TS-as-spec'd, full compatibility
- **AOT compile typed code** to native (or wasm); JIT/interpret dynamic code
- Static-typed regions are GC-free (stack-allocated, escape-analyzed)
- Dynamic regions (`any`, untyped closures, `eval`) live in a small GC heap
- Like Hermes (Facebook's RN engine) but with more aggressive AOT

**Pros**: keeps full TS compat, gets the perf advantage on the well-typed common case. Realistic compromise.

**Cons**: complex implementation — two memory models living together, GC/no-GC interop across the boundary. The boundary itself is a perf cliff and a correctness hazard. Conceptually fragile.

### Position C — Radical (AOT-only, no-GC, language redesign)

- TS **syntax** preserved, **semantics** modified to support static ownership
- AOT compilation only — no interpreter, no JIT
- Borrow checker (or affine types, or region inference)
- No GC. Drop, like Rust.
- `tsc`-compatible TS code that uses dynamic features (closures with mutable captures, sharing) **does not run** unless rewritten or annotated
- `eval` and dynamic `import` not supported
- This is *literally Rust with TS syntax + structural typing*

**Pros**:
- Strongest perf ceiling — same as Rust/C++/Zig (because it *is* that)
- Strongest research interest — no other major language is "Rust with TS syntax + structural types + nominal types as opt-in"
- Clear identity — easy to describe: "if Rust let you write structural types, that's torajs"
- AOT-to-native and AOT-to-wasm both natural; playground fits perfectly
- Aligned with the "极致 perf, beat Bun" goal — not by being a faster JS engine, but by being a *different category of thing*

**Cons**:
- We're designing a language, not implementing one. Multi-year language design effort before the first useful program runs.
- Programs that run on Bun won't run on torajs. We can't say "drop-in TS replacement" — must say "TS-syntax language with stricter semantics".
- The compiler is the project. This is closer to early Rust development than to early Bun development.
- Borrow checking is a notoriously hard piece of language engineering. Rust took 7+ years of design before stabilizing.

## Consequences for the staged roadmap (0002)

The Position-A roadmap (interpreter → bytecode → JIT) is what 0002 wrote. If we pick **B** or **C**, that roadmap is **wrong**:

| Position | What replaces "interpreter → bytecode → JIT" |
| --- | --- |
| A | (no change) interpreter → bytecode VM → JIT, with type-directed perf wins layered in |
| B | interpreter for development; AOT compiler for typed code from v1; GC for dynamic regions |
| C | **no interpreter at all**. The "walking skeleton" becomes "minimum AOT compiler + emitter". V0 is `tr build hello.ts → hello.wasm` running `console.log("hello")`. Different shape. |

The Position-C v0 looks roughly like:
1. Lexer (TS-shaped)
2. Parser → typed AST
3. Type checker (structural + ownership/borrow)
4. Lowering: AST → some IR (probably SSA)
5. Codegen: IR → wasm bytes (use cranelift's wasm target, or hand-written)
6. CLI: `tr build foo.ts → foo.wasm; tr run foo.wasm` (with a tiny wasm host)

That's bigger than the Position-A walking skeleton, but it's also **closer to the end goal**. No throwaway interpreter; no JIT to plan around; no GC to design and replace.

## What aligns best with the stated goals?

Recapping the stated goals:
1. 极致 perf, beat Bun, hold key benchmarks
2. Pure research, no external customer
3. WASM as a first-class target (for torajs.com playground)
4. TS-native, not transpile-then-run
5. Closed-source / internal — no compat pressure

**Position C scores highest on every one of these.** Specifically:
- Goal 1 — C's perf ceiling is C/Rust-class, categorically above any GC'd JIT'd language.
- Goal 2 — research project; designing a language is the most research-shaped possible activity.
- Goal 3 — AOT-to-wasm is the natural compilation target; no runtime needed in the browser.
- Goal 4 — there is no JS at all in C. TS syntax all the way through.
- Goal 5 — no compat means we can break the rules of TS that force GC.

Position A is what we'd pick if torajs were a *product* with TS-shop users. It isn't. Position B is the hardest engineering and the least clear identity. Position C is the most coherent.

The cost of C is honest: **multi-year language design**, not implementation. We don't ship a runtime in 6 months; we ship a working compiler in 12-18 months that maybe implements 10% of the TS surface, and we extend it from there. The first useful program is bigger than "hello world".

But: walking-skeleton-on-position-C is still very small. ~500-1000 lines of Rust to get `tr build hello.ts → hello.wasm` running. Different surface (a wasm emitter instead of an evaluator), but comparable scope.

## Decision points

I want explicit answers, because everything below this is downstream.

### D1 — Pick a position

> takagi: A / B / C / something else?

If C, all of the following follow:

### D2 — Backend choice for the AOT compiler

| Backend | Compile speed | Output speed | Effort | Wasm target |
| --- | --- | --- | --- | --- |
| **Cranelift** | fast | good (Bun-class) | medium | yes (`cranelift-wasm` exists) |
| **LLVM** | slow | best | high (lots of glue) | yes (LLVM wasm backend) |
| **Custom** | varies | varies | very high | DIY |
| **Direct wasm emit (no IR)** | fastest | mediocre | lowest | yes — write `[u8]` |

I lean **direct wasm emit for v0**, swap in Cranelift around v2. Direct wasm emit means writing wasm bytes by hand from our own IR — feasible because wasm's binary format is simple. It locks us out of native targets temporarily, but we said wasm is first-class.

### D3 — Ownership model

| Model | Examples | Cognitive load on user | Implementation difficulty |
| --- | --- | --- | --- |
| **Borrow-checked (Rust-style)** | Rust | high — users must learn lifetimes | very high |
| **Affine types (move-only, no borrows)** | Mercury, parts of Linear Haskell | medium | medium-high |
| **Region inference** | Cyclone, MLton's regions | low (mostly invisible) | high (research-level) |
| **Refcount + cycle collector** (still no GC, deterministic) | quickjs, Swift ARC | low | low |

I lean **affine + explicit regions** for the start, with borrow checking as a stretch goal. This gives 80% of Rust's ownership win without the lifetime annotation tax. Refcounting is a fallback if affine proves too restrictive.

### D4 — `any` policy

`any` in TS escapes the type system. In a no-GC world, that's a hole — no static lifetime tracking possible.

- **Ban it**: easiest. Forces fully typed programs. Loses gradual-typing convenience.
- **Box it**: `any` becomes a refcounted box. Programs can use it but pay ARC overhead.
- **Forbid `any` in compiled programs, allow it in REPL only**: hybrid for tooling.

I lean **ban it for compiled programs**.

## What v0 looks like under Position C

`labs/0001-walking-skeleton/` — a Rust binary `tr` that:

1. Reads `hello.ts` containing `console.log("hello")`
2. Lexes it (1 file, ~150 LOC)
3. Parses it to a tiny AST (1 file, ~150 LOC)
4. Type-checks it trivially (`console.log`'s signature accepts `string`) (1 file, ~50 LOC)
5. Lowers it to a tiny IR (1 file, ~100 LOC)
6. Emits a wasm module that imports `print` and calls it (1 file, ~200 LOC) — direct wasm emit, no Cranelift
7. CLI: `tr build hello.ts → hello.wasm; tr run hello.wasm` (1 file, ~100 LOC)
8. Runtime host: tiny wasm executor (use `wasmi` or `wasmtime` as a Rust dep) that supplies `print`

Total: ~750-1000 LOC, in `labs/0001-walking-skeleton/`. No GC; no closures yet (so no captures to worry about); no allocations (the only "string" lives in the wasm data section).

This is a **bigger v0 than 0002's tree-walker** by ~2×, but it's directly on the production trajectory — every line is part of the real engine, not throwaway tree-walking.

## Out of scope for this doc

- Specifying the actual ownership model rules (D3 needs its own RFC once decided)
- The full type system — structural? nominal? both? gradual?
- Module system
- Standard library (what's in `console.log`, anyway?)

## Next step

D1 first. Everything else is downstream.
