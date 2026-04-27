# 0002 — Engine architecture for maximum performance

**Status**: open discussion
**Started**: 2026-04-26
**Depends on**: `0001-direction.md` (TS-native, Rust, research project, beat Bun)
**Question**: given the goal "极致性能, 超过 bun/nodejs, 一直保持 benchmark", how do we design the engine from day 1 so we don't paint ourselves into a corner?

## The fundamental sources of perf in a runtime

Performance is **multiplicative across layers**. A 2× improvement at one layer composes with a 2× at another to give 4×. Skipping a layer (e.g. no IC, no JIT) caps the whole stack.

Layers, in order of impact:

1. **Value representation** — every operation touches values
2. **Object / property model** — every property access goes through it
3. **Execution strategy** — interpreter vs bytecode VM vs JIT vs AOT (5-10× per step up)
4. **GC strategy** — pauses, allocator pressure, hot-path interaction
5. **Inline caches / specialization** — speeds up dynamic dispatch
6. **Memory layout** — cache locality, pointer chasing, frame layout

And for TS-native specifically, a 7th layer that JS engines **cannot have**:

7. **Type-directed lowering** — types from TS skip dispatch entirely

(7) is the structural advantage. Without it, "beat Bun" is "be a slightly faster JSC clone" which is unwinnable. With it, we're competing in a category JS engines are excluded from by their input language.

## Layer-by-layer choices

### L1 — Value representation

| Option | Size | Speed | Notes |
| --- | --- | --- | --- |
| `enum Value { Num(f64), Str(Rc<String>), ... }` | 24 B | slow (tag dispatch on every op) | what most prototypes do; locks out perf |
| **NaN-boxing** | 8 B | fast (numbers bare; others encoded in NaN payload) | JSC, SpiderMonkey. Real f64 ops with no overhead. |
| Tagged pointers (low-bit tags) | 8 B | fast (SMI fastpath, others tagged ptr) | V8. Slightly worse for floats than NaN-boxing. |
| Pointer compression | 4 B (in 64-bit space) | fast + half memory | V8 add-on. Stackable on top of tagging. |

**Recommendation**: NaN-boxing from v0. Numbers are bare f64s (zero overhead for arithmetic), pointers/strings/booleans/null/undefined go in NaN payload bits. ~50 lines of unsafe Rust to get right.

**TS-native bonus**: when a slot's type is known statically (e.g. `let x: number`), we can store it as a bare f64 outside any boxing scheme. Boxing is only for `any` / union types / heap pointers. This is the cheapest big win.

### L2 — Object / property model

JS-engine standard:
- **Shapes** (a.k.a. hidden classes) — objects with the same property names+order share a layout descriptor. Property access is "load shape ptr → look up offset → load slot at offset".
- **Inline caches** — first call site execution is uncached; after observation, monomorphic IC stores `(shape_id, offset)` and skips the lookup; polymorphic IC stores 2-4; >4 = megamorphic, fallback to hashmap.
- **Inline storage** — first N (8-12) properties live inline in the object header; rest spill to an out-of-line array.

**TS-native bonus**: typed object literals like `{ x: 1, y: 2 }: Point` have **statically known** layout. We emit struct-style access (`load offset 0` for `.x`) with no shape lookup, no IC, no fallback. This is real C-struct-class perf for typed code.

For `any`-shaped objects: fall back to full shape + IC machinery. Years of work, but well-trodden.

### L3 — Execution strategy

| Stage | Dispatch | Effective MIPS | Real-world example |
| --- | --- | --- | --- |
| Tree-walker | match on AST node | 1-10 | most teaching interpreters |
| Switch-dispatched bytecode VM | `match opcode` in a loop | 10-100 | quickjs, CPython 3.10 |
| Threaded code / computed-goto | tail-calls between handlers | 30-300 | wasm3, CPython 3.11+ |
| Baseline JIT | compile to native, no opt | 100-1000 | JSC Baseline, V8 Sparkplug |
| Optimizing JIT | type feedback, inlining, escape analysis | 1000+ | JSC DFG/FTL, V8 TurboFan |
| **AOT (typed)** | compile to native at parse time | C-class | possible only because we have types |

JS engines stop at "optimizing JIT" because their input is untyped — they need runtime feedback to specialize. **We have types statically, so we can AOT typed code paths.** That's where torajs's perf ceiling lives.

Path: tree-walker (v0) → bytecode VM (v1) → bytecode + AOT typed lowering (v2) → baseline JIT for dynamic code (v3) → optimizing JIT (much later, maybe never if AOT covers enough).

### L4 — GC

| Strategy | Pause | Overhead | Cycle leaks | Implementation cost |
| --- | --- | --- | --- | --- |
| Reference counting | predictable, small | atomic incr/decr on every assign | yes, unless cycle collector | low |
| Mark-sweep | scales with heap | low when not running | no | medium |
| Generational | scales with nursery | low; nursery promotes only survivors | no | medium-high |
| Concurrent / incremental | small | high | no | very high |

JS engines all use generational + concurrent. We probably want there too eventually.

**v0 expedient**: refcount with `Rc<T>` for heap values. Has cycle leaks. Acceptable for walking-skeleton phase. Replaceable later because *the rest of the engine should not assume refcounting* — interact with values through a Value type that hides ownership.

**TS-native bonus**: escape analysis is dramatically easier with types. Many object allocations become stack-allocated. This is a real win, free from the type system.

### L5 — ICs / specialization

ICs only matter once we have a stable bytecode (instruction site to attach the cache to). Defer until L3 hits bytecode-VM stage.

For typed code: ICs are mostly redundant — we already know what shape we're hitting. ICs only kick in for `any` and untyped JS-shaped corners.

### L6 — Memory layout

Boring engineering, mostly invisible until profiling:
- Bytecode buffer: tightly packed `[u8]`, no per-instruction allocation
- Dispatch table: inline-able array, cacheline-aligned
- Stack frames: fixed-layout struct, fields ordered by access frequency
- AST/IR: **arena-allocated, indexed by `NodeId`**, never `Rc<RefCell<...>>` graphs (forces pointer chasing, locks out parallelism)

### L7 — Type-directed lowering (the structural moat)

The big idea. Given:

```ts
function add(a: number, b: number): number { return a + b }
add(1, 2)
```

JS engine has to (each runtime call):
1. Check `a` is a number (or string, or object with valueOf, or...)
2. Check `b` is a number (or...)
3. Choose between integer add, float add, string concat, coerce, etc.
4. Possibly box the result

Torajs emits, at parse time, with type info propagated:
```
fadd_local a, b → return
```

Two `f64` adds, one return. Same as C `double add(double a, double b) { return a + b; }`.

**Generalization**: every typed expression is lowered with full knowledge. `arr[i]` where `arr: number[]` becomes a bounds check + load. `obj.x` where `obj: Point` becomes a constant-offset load. `if (cond)` where `cond: boolean` skips the truthy-coercion machinery.

This is the structural perf advantage. JS engines cannot do this because they don't have the types.

For untyped (`any`) regions: degrade to standard JS-engine machinery. We won't beat Bun on `eval('1+1')`. We'll beat Bun on `function fib(n: number): number { return n < 2 ? n : fib(n-1) + fib(n-2) }`.

## What this implies for v0

The first-version commitments that **don't lock us out** of the above:

| Commit | Rationale |
| --- | --- |
| **NaN-boxed Value** | swapping value rep later means touching every operation; do it once, up front |
| **Arena AST**, `NodeId`-indexed, immutable | enables passes (type checking, lowering, serialization) without graph rewriting |
| **Type info attached to AST nodes from the parser** | even if v0 ignores types at runtime, the data is there for v1+ to use |
| **`Value` is an opaque type with method-only access** | lets us swap representations without touching call sites |
| **Bytecode-shaped interpreter loop** even when tree-walking | structure as `match (op, args)` not deep recursion; lowers to bytecode mechanically later |
| **No `Rc<dyn Any>` for runtime objects** | the dyn-trait dispatch tax is permanent if we let it in |

The first-version **deferrals** (start simple, replace later):

| Defer | Reason |
| --- | --- |
| Shapes / hidden classes | hashmap for property storage in v0; no perf budget needed yet |
| ICs | nothing to attach them to until bytecode VM |
| GC (use Rc with cycle leaks) | replaceable if `Value` hides ownership |
| Module loader, async, full stdlib | single-file sync execution in v0 |
| Type checker | erased types in v0, runtime ignores them; checker comes in v1 |
| AOT codegen | parser → bytecode → interpreter is the spine; AOT layers on top |

## The staged perf roadmap

| Stage | Execution | Value | Object | Types at runtime | Perf class |
| --- | --- | --- | --- | --- | --- |
| 0 — walking skeleton | Tree-walker | NaN-boxed | hashmap | erased | exists; ~quickjs ÷ 5 |
| 1 — bytecode VM | switch dispatch | NaN-boxed | shapes (basic) | erased + checked | quickjs-class |
| 2 — typed fast path | bytecode + typed-slot lowering | NaN-boxed + bare typed slots | shape-cached + struct | type-directed lowering | beats quickjs significantly |
| 3 — baseline JIT | method JIT for dynamic | + ICs | + IC fallback | feedback-driven | Bun-class on dynamic code |
| 4 — AOT typed lowering | typed code → native at parse | bare types | direct struct | full | beats Bun on typed code |
| 5 — optimizing JIT | TurboFan-class for any-shaped hot code | full | full | full | beats V8/JSC on typed |

This is years of work. Most of it lives in `labs/` first.

## Tensions to resolve

A few decisions where reasonable people would disagree, and I want explicit user input:

### T1 — Commit to NaN-boxing in v0, or hack `enum Value` and rewrite?

- **Commit**: ~50 lines of unsafe in v0; never touch it again; every layer above stacks cleanly.
- **Hack**: 10 lines of safe `enum Value` in v0; rewrite later when bytecode VM lands; touches every operation in the interpreter to migrate.

I lean **commit**. The unsafe is contained. Rewriting value rep is a known nightmare in compiler projects.

> takagi: ___

### T2 — Typed fast path in v0, or wait?

- **In v0**: even the walking skeleton uses type info — typed `number` locals stored as bare f64s outside the boxed Value. Doubles the v0 implementation effort but proves the unique angle of torajs immediately.
- **Wait**: v0 ignores types; types kick in at v2 when bytecode lands. Cleaner staging, but the first version doesn't yet show *why* this engine is interesting.

I lean **wait** — get the spine in first, then layer in typed lowering. The angle is the design, not the v0 demo.

> takagi: ___

### T3 — GC strategy decision now, or defer?

- **Decide now**: pick generational tracing GC, design Value/object model around it from day 1. High up-front cost; hard to walking-skeleton.
- **Defer**: refcount in v0, swap later. Risk: refcounting habits leak into the codebase.

I lean **defer** — but with a strict invariant: nothing outside the Value module knows it's refcounted. All ownership flows through `clone_value()` / `drop_value()` style methods. The day we swap in tracing GC, those become no-ops.

> takagi: ___

### T4 — Frontend: hand-written parser, or generated / parser-combinator?

- **Hand-written recursive descent**: what tsc, swc, oxc, V8 all use. Best perf, full control, painful to change.
- **Parser combinator (e.g. `chumsky`, `nom`)**: faster to write, easier to evolve early. ~3× slower at runtime; doesn't matter at v0 but might at v3.
- **Generated (e.g. ANTLR / lalrpop)**: speed in between, depends on tool.

For TS specifically, **only hand-written has worked at scale** — TS's grammar has too many ambiguities (`<T>` JSX vs cast vs generic, `(x as Foo)` vs invocation, ASI). swc and oxc both abandoned PEG / generators after early prototypes.

I lean **hand-written from day 1**, knowing it's painful, because every other choice has been tried by smarter teams and abandoned.

> takagi: ___

## Out of scope for this doc (defer)

- Module resolution and the loader pipeline
- Async / event loop / promises
- FFI to Rust (separate doc when we get there)
- Threading model — Worker-shaped isolates, or shared memory? (much later)
- Debugger / source maps — important but not load-bearing for perf

## Next step

Resolve T1-T4. Then either:

(a) Open `labs/0001-walking-skeleton/` with the v0 commitments locked in, or
(b) Open `0003-walking-skeleton-spec.md` with concrete file/module breakdown before any code.

Both are fine; (b) is one more discussion turn, (a) starts code.
