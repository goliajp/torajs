# torajs — language status

torajs implements TypeScript with TS semantics, targeting the same
observable behavior as `bun` runs the same source. The reference
baseline is bun: anything bun runs, tr eventually runs. Anything
not yet supported is a roadmap phase, not a design boundary.

This doc describes **what works today** and **what's planned** with
links to the relevant roadmap phases.

## Contract: `bun` is the oracle

When behavior is unclear, write the equivalent in TS, run it in
`bun`, and match. If torajs differs from bun's output (excluding
documented perf differentiators below), that's a bug — file an issue.

## Currently working

### Types
- Primitives: `number` (i64 default; f64 when an arg lowers to f64
  — Math intrinsics, decimal literals, division), `boolean`,
  `string`, `void`, `null`, `Nullable<T>` (`T | null`)
- Object literal types: `type Point = { x: number, y: number }` —
  structural, declaration-order layout
- Homogeneous arrays: `T[]`
- Class declarations: instance + static fields/methods, inheritance,
  abstract classes, visibility modifiers (`private` / `protected` /
  `readonly`)
- Generics: `function f<T>(x: T): T { ... }` and `type Pair<A, B> =
  { ... }` — monomorphized per call site
- `any` / untyped fn params — auto-promoted to fresh type-params and
  monomorphized at call sites; same path for explicit `: any`
- Function types: `(args) => R` / `__fn(args)->R`

### Statements
- `let` / `const` declarations (with or without type annotations
  — annotations infer from the init expression for primitive cases)
- `let x;` / `let x: T;` (uninitialized) — first follow-up assignment
  in scope provides the init
- `if` / `else`, `while`, `do-while`, `for`, `for-of`
- `try` / `catch` / `finally` / `throw`
- `function` declarations (top-level + methods)
- `function (params): R { body }` — function expressions in any
  expression position
- `class` declarations (full feature set above)
- Block scope `{ ... }`
- `import` / `export` (named imports across files)
- Generator functions (`function* g(): T`) and `yield` / `yield *`

### Expressions
- Number arithmetic (`+ - * / % ** **=`)
- Bitwise (`& | ^ << >> >>>`)
- Comparison (`< > <= >= === !==`)
- Logical (`&& || !`)
- String concatenation (`+`)
- Template literals (`` `foo ${expr} bar` ``)
- Member access `obj.field`
- Index access `arr[i]`
- Function calls (incl. variadic `console.log` / `JSON.stringify`)
- Object literals `{ x: 1, y: 2 }` with spread members
- Array literals `[1, 2, 3]` with spread
- `new ClassName(...args)`
- Closures with captures (lifted to top-level FnDecls)
- Optional chaining `obj?.field` / `obj?.[i]` / `obj?.fn()`
- Nullish coalescing (basic forms via Nullable)
- `arguments.length` / `arguments[N]` (literal index) inside
  non-arrow function bodies — the broader `arguments` object is a
  follow-up

### Standard library
- `console.{log, error, warn}(...args)` — variadic, accepts any
  printable value
- `Math.{abs, sqrt, floor, ceil, round, log, log2, log10, exp, exp2,
  pow, min, max, sin, cos, tan, asin, acos, atan, atan2, sinh, cosh,
  tanh, asinh, acosh, atanh, expm1, log1p, imul, clz32, fround, hypot,
  cbrt, trunc, sign, random}` + `Math.{PI, E, LN2, LN10, LOG2E, LOG10E,
  SQRT2, SQRT1_2}`
- `Number.{parseInt, parseFloat, isNaN, isFinite, isInteger}` +
  `Number.{MAX_VALUE, MIN_VALUE, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER,
  POSITIVE_INFINITY, NEGATIVE_INFINITY, EPSILON, NaN}`
- `String.fromCharCode` / `String.fromCodePoint` (variadic)
- String methods: `length, slice, substring, repeat, toUpperCase,
  toLowerCase, trim{,Start,End,Left,Right}, padStart, padEnd, replace,
  replaceAll, charAt, at, charCodeAt, codePointAt, startsWith,
  endsWith, includes, indexOf, lastIndexOf, localeCompare, split, join`
- Array methods: `length, push, pop, shift, unshift, slice, splice,
  concat, indexOf, lastIndexOf, includes, find, findLast, findIndex,
  findLastIndex, some, every, map, filter, forEach, flatMap, reduce,
  reduceRight, sort, reverse, fill, at, join`
- `JSON.stringify(value)` and `JSON.parse(text)` (with caller-driven
  type inference for class / array / nested object shapes)
- `Object.keys(obj)`, `Object.assign(target, source)`

### Memory
TS reference semantics — heap-allocated values (strings, objects,
arrays, closures) follow ordinary aliasing rules. `let q = p; p.x`
is just fine; both names refer to the same object. The runtime uses
ARC (refcount) for heap-owned types under a universal heap header;
this is an implementation detail invisible to the user.

## Roadmap items (not yet implemented)

Each unimplemented TS feature lives in a roadmap phase. The phase IDs
below correspond to entries in `docs/roadmap.md`.

| Feature | Phase | Status |
|---|---|---|
| `arguments` object (full materialization) | M-arguments | partial — only `arguments.length` / `arguments[N]` literal-index rewrites |
| `Object.{getPrototypeOf, getOwnPropertyDescriptor, defineProperty, freeze, ...}` | M-stdlib-object | not started |
| `Symbol`, `Proxy`, `WeakMap`, `WeakSet`, `WeakRef` | M-meta | not started — gated on metadata machinery |
| Regex literal `/...../` and `new RegExp(...)` | M-regex | not started |
| `BigInt` | M-bigint | not started |
| `async` / `await` / `Promise` | M7 | not started |
| Microtask queue + top-level `await` | L.3 / L.4 | not started |
| `eval` / `Function` constructor | M-dynamic | substrate-dependent, not in v0.1 |
| ESM `import x from "./y"` (default), `import * as ns`, side-effect imports | K.9 | not started — only named imports today |
| `Date` | M6.3-date | not started |
| `fs` / `Bun.file` | M6.4 / M6.5 | not started |
| Mutable refcount globals (`let xs: T[] = []; xs.push(...)` at top level) | K.8 | not started — workaround: wrap top-level driver in a `main()` |
| Top-level `await` | L.4 | not started |
| Decorators | M-decorators | not in v0.1 |
| Mapped / conditional types | M-types-advanced | not in v0.1 |
| `JSON.stringify(value, replacer, indent)` indent-aware emission | M-json-pretty | parsed but not pretty — emits compact form |
| Array of fully-mixed types | M-tuple | not started — current arrays are homogeneous |
| Function statement hoisting (nested `function` inside blocks) | M-hoist | not started |

## Differences from bun (intentional)

- **AOT to native binary**: `tr build` produces a small statically-
  linked binary; bun bundles its V8 runtime.
- **Cold-start time**: torajs starts in ~1.2 ms; bun in ~7–8 ms;
  node-v8 in ~80 ms.
- **Bench scoreboard**: tr wins on all 19 committed bench cases
  (see `docs/perf.md`).
- **Type checking is real**: torajs typechecks the full program;
  untyped TS that bun runs without complaint may produce a
  type-error rejection in tr until the corresponding inference path
  lands (most untyped patterns are already covered — see the
  Currently working list above).

## How to know if your TS works in torajs

1. `tr check yourfile.ts` — typecheck only, exits non-zero on error.
2. If it passes, `tr run yourfile.ts` — compiles + caches + executes.
3. For deployment perf, `tr build yourfile.ts -o yourbinary`.
4. Compare against bun: `diff <(bun run x.ts) <(tr run x.ts)` should be empty.

If `tr` rejects something `bun` accepts, that's a roadmap-phase gap.
Check the table above for the relevant phase, and file an issue if
there's no entry.
