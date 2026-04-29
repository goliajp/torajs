# torajs — the TypeScript subset

torajs implements **part of TypeScript** with **TS semantics** — the same observable behavior as bun running the same source. We do not import foreign concepts (Rust ownership, RefCell, manual `.clone()`). The differentiator is the runtime: AOT to native binary via LLVM, or JIT via Cranelift, no tracing GC, no refcount, with **compile-time ownership inference** silently handling memory.

This doc enumerates what's in the subset and what isn't.

## Contract: `bun` is the oracle

When behavior is unclear, write the equivalent in TS, run it in `bun`, and match. If torajs differs from bun's output (excluding our perf differentiators), that's a bug.

## In the subset

### Types
- Primitives: `number` (i64 by default; opt in to `f64` with explicit annotation), `boolean`, `string`, `void`
- Object literal types: `type Point = { x: number, y: number }` — structural, declaration-order layout
- Homogeneous arrays: `T[]` (in progress; runtime in M2)

### Statements
- `let` / `const` declarations with type annotations
- `if` / `else` (block bodies)
- `while` (block bodies)
- Block scope `{ ... }`
- Function declarations with `function` keyword
- Arrow functions `(...) => expr` / `(...) => { ... }`
- `return` statements
- `type` aliases for structural types

### Expressions
- Number arithmetic (`+ - * / %`)
- Bitwise (`& | ^ << >>`)
- Comparison (`< > <= >= === !==`)
- String concatenation `a + b`
- Boolean operations (planned)
- Member access `obj.field`
- Index access `arr[i]`
- Function calls
- Object literals `{ x: 1, y: 2 }`
- Array literals `[1, 2, 3]`

### Standard library (slice 1)
- `console.log(value)` — prints any value to stdout with newline
- `Math.{sqrt, abs, floor, ceil, log, exp, pow, min, max}` + `Math.{PI, E}` constants
- `String.length`

### Memory & shared references — TS-shape, no runtime cost

Heap-allocated values (strings, objects, arrays) follow TS reference semantics:

```ts
let s: string = "hello";
let n: string = s;       // both bindings reference the same heap
console.log(s);          // prints "hello"
console.log(n);          // prints "hello"
                          // ↑ scope ends, one drop fires (n is the owner;
                          //   s transferred at the let)
```

```ts
let p: Point = { x: 1, y: 2 };
let q: Point = p;        // alias
let n: number = p.x;     // ok — read through aliased binding
let m: number = q.y;     // ok
                          // scope end, one drop (q owns the struct)
```

```ts
let alice: Person = { name: "alice", age: 30 };
let bob: Person = alice;
console.log(alice.name);  // ok
console.log(bob.name);    // ok
                           // scope end, one drop (bob owns; recursively
                           // frees the struct + the inner string)
```

**Implementation:** the compiler tracks per-binding ownership at compile time. `let n = s` transfers ownership from s to n; both slots remain readable (the heap pointer is in both). At scope exit, the drop fires on the current owner (n); the alias slot (s) is skipped.

## NOT in the subset (compile-time rejected)

### Multi-rooted ownership across transfers

```ts
let a: string = "x";
let b: string = a;
let c: string = a;    // ✗ "cannot transfer `a` — value was already aliased"
```

**Why:** after `let b = a`, both `a` and `b` reference the same heap. Transferring `a` again into `c` would create three aliases with no clear owner — the compiler can't statically resolve which scope's end fires the drop. With no refcount or GC fallback, this is rejected.

**Workaround:** transfer from the most recent binding.

```ts
let a: string = "x";
let b: string = a;
let c: string = b;    // ✓ transfers from b, the current owner
```

### Multi-rooted ownership into a struct

```ts
let s: string = "hello";
let n: string = s;
let c: Container = { name: s };  // ✗ "cannot transfer `s`"
```

**Workaround:** transfer from the alias instead.

```ts
let s: string = "hello";
let n: string = s;
let c: Container = { name: n };  // ✓
```

### Transfer to inner scope, read from outer (silent UAF, planned compile-error)

```ts
let s: string = "hello";
{
  let n: string = s;
}
console.log(s);  // ⚠ undefined behavior in current torajs (heap freed by n's drop)
```

**Status:** the compiler currently doesn't catch this — runs into use-after-free at runtime. Will become a compile error when escape-analysis lands (M2+). Until then, write code that doesn't transfer into an inner scope and then read from outer.

### Other TS features not in the subset

- `null` / `undefined` — dropped by design
- `==` / `!=` — only `===` / `!==`
- `var` keyword — only `let` / `const`
- Decorators — not planned
- `eval` / `Function` constructor — not planned
- JSX — out of scope
- Test262 conformance — out of scope (we are a subset)
- Class syntax — possibly later as desugaring
- Generics (in user code) — M3 milestone
- `try` / `catch` / `throw` — M4 milestone
- Async / await — M7 milestone
- ESM imports / multi-file — M6 milestone
- Closures with captures — M5 milestone

## Differences from bun (intentional)

- **Type checking is real**: torajs requires type annotations (no implicit `any`); bun runs un-typed JS.
- **Compile to native**: `tr build` produces a 30-something-KB statically-linked native binary; bun bundles its full V8 runtime.
- **Cold-start time**: torajs starts in ~1.2 ms; bun in ~7-8 ms; node-v8 in ~80 ms.
- **No GC, no refcount**: deterministic memory management via compile-time ownership inference.

## Differences from bun (work in progress)

- Stdlib is much smaller (slice 1 only). `Bun.*` namespace, `fs`, `http`, `fetch` etc. roll out across M6+.
- No module system yet (M6).
- No closures yet (M5) — workaround: top-level functions only.
- No async (M7).

## How to know if your TS works in torajs

1. Read the "in the subset" list above.
2. Run `tr check yourfile.tora.ts` — typechecks fast (~1ms).
3. If it passes, run `tr run yourfile.tora.ts` — JIT-executes with the Cranelift backend.
4. For perf, build with `tr build yourfile.tora.ts -o yourbinary`.

If `tr check` errors with `cannot transfer X — value was already aliased earlier`, restructure to transfer from the most recent binding (see the workarounds above).
