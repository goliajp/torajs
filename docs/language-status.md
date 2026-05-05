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
  endsWith, includes, indexOf, lastIndexOf, localeCompare, split, join,
  normalize, matchAll, match`
- Array methods: `length, push, pop, shift, unshift, slice, splice,
  concat, indexOf, lastIndexOf, includes, find, findLast, findIndex,
  findLastIndex, some, every, map, filter, forEach, flatMap, flat,
  reduce, reduceRight, sort, reverse, fill, at, join` +
  `Array.from / Array.of`
- `JSON.stringify(value)` and `JSON.parse(text)` (with caller-driven
  type inference for class / array / nested object shapes)
- `Object.{keys, values, hasOwn, is, getOwnPropertyNames, assign}`
  (assign is single-source MVP)
- **Regex (v0.2 #1)**: literal `/pattern/flags` + `new RegExp(...)`;
  flags `g i m s y` (full `u` Unicode property escapes deferred to v1.0);
  methods: `re.test(s)`, `re.exec(s)`, `s.match(re)`, `s.matchAll(re)`,
  `s.replace(re, repl)`, `s.replaceAll(re, repl)`, `s.split(re)`;
  NFA → DFA in `runtime_regex.c` (1760 LOC, self-hosted; pillar 2 自研)
- **Date (v0.2 #2)**: full constructor arity + ISO 8601 round-trip;
  `getFullYear / getMonth / getDate / getHours / getMinutes /
  getSeconds / getMilliseconds / getDay / getTime / valueOf /
  toISOString / toString` (local + UTC variants); static
  `Date.now() / Date.parse() / Date.UTC()`
- **fs sync (v0.3 #1)**: `readFileSync, writeFileSync, appendFileSync,
  unlinkSync, mkdirSync, existsSync` (async forms gate on v0.5)
- **Bun namespace (v0.3 #2)**: `Bun.write(path, data)`, `Bun.argv`
  (`Bun.file(p)` async API gates on v0.5)
- **process surface (v0.3 #3)**: `process.{argv, env, env.NAME,
  platform, cwd, exit}` (`process.{stdout, stderr}.write` /
  `process.stdin.read` planned T-03 of v0.3.0)

### Memory
TS reference semantics — heap-allocated values (strings, objects,
arrays, closures) follow ordinary aliasing rules. `let q = p; p.x`
is just fine; both names refer to the same object. The runtime uses
ARC (refcount) for heap-owned types under a universal heap header;
this is an implementation detail invisible to the user.

## Roadmap items (not yet implemented)

Each unimplemented TS feature lives in a roadmap phase. The T-IDs
below correspond to the 33-item linear plan in `docs/roadmap.md` →
"Roadmap v2: 33-item linear plan".

| Feature | Phase | Status |
|---|---|---|
| `JSON.parse` f64 path (caller-typed) | T-02 (v0.3.0) | substrate ready (`__torajs_json_parse_float` in `runtime_str.c`); needs caller-driven typing wired + fixture |
| `process.{stdout, stderr}.write` + `process.stdin.read` (sync) | T-03 (v0.3.0) | not started |
| `tr fmt` deterministic source reformatter | T-05 (v0.3.0) | not started |
| `tr lint` (5 starting rules) | T-06 (v0.3.0) | not started — depends on T-04 (Checker.errors → Vec<(Span, Severity, String)>) |
| Object stdlib completion (`entries / freeze / isFrozen / getPrototypeOf / setPrototypeOf / defineProperty / defineProperties / getOwnPropertyDescriptor / fromEntries`) | T-09 (v0.4.0) | not started |
| `Type::Any` boxing substrate | T-10 (v0.4.0) | not started — unlocks heterogeneous arrays + `arguments` + `Function` ctor |
| `arguments` full materialization (dynamic index, `arguments.callee`, runtime heterogeneous array) | T-11 (v0.4.0) | partial — only `arguments.length` / `arguments[N]` literal-index + `[...arguments]` spread |
| `String.raw` + template literal raw-strings array | T-12 (v0.4.0) | not started |
| `Symbol` / `Symbol.iterator` / `Symbol.asyncIterator` / `Symbol.toPrimitive` / `Symbol.for` | T-13 (v0.4.0) | not started |
| `Type::Promise<T>` | T-14 (v0.5.0) | not started |
| Single-thread executor (Tokio-shape) | T-15 (v0.5.0) | not started |
| `async` / `await` state-machine lowering | T-16 (v0.5.0) | not started |
| `Promise.{all, race, allSettled, any, resolve, reject}` | T-17 (v0.5.0) | not started |
| fs async (`readFile / writeFile / readdir / stat / unlink / mkdir / append`) | T-18 (v0.5.0) | not started — gates on v0.5 async/await |
| `Bun.file(p).text() / .arrayBuffer() / .json()` | T-19 (v0.5.0) | not started — gates on v0.5 |
| wasm32-wasi target | T-20 (v0.6.0) | not started |
| `fetch` (HTTP) | T-21 (v0.6.0) | not started |
| Playground UI (Monaco + share-link) | T-22 (v0.6.0) | not started |
| vtable upgrade for virtual dispatch (currently tag-switch) | T-24 (v1.0.0) | not started — vtable_ptr slot already reserved |
| `BigInt` (self-hosted arbitrary precision) | T-25 (v1.0.0) | not started |
| `WeakRef` / `WeakMap` / `WeakSet` + ARC-aware cycle collector | T-26 (v1.0.0) | not started |
| `Function` constructor / `eval` | T-27 (v1.0.0) | not started — depends on T-10 |
| Multi-platform release (linux-x86_64 / aarch64, windows-x86_64) | T-28 (v1.0.0) | not started |
| `tr debug` step debugger (DWARF + DAP) | T-29 (v1.0.0) | not started — DWARF substrate shipped v0.3 #4 |
| `tr repl` interactive loop | T-30 (v1.0.0) | not started |
| `libtora.a` + `tora_eval()` embedding API | T-31 (v1.0.0) | not started |
| ESM `import x from "./y"` (default), `import * as ns`, side-effect imports | post-v1.0 polish | not started — only named imports today |
| Mutable refcount globals (`let xs: T[] = []; xs.push(...)` at top level) | post-v1.0 polish | not started — workaround: wrap top-level driver in a `main()` |
| `JSON.stringify(value, replacer, indent)` indent-aware emission | post-v1.0 polish | parsed but not pretty — emits compact form |
| Array of fully-mixed types (tuple) | (not in v1.0 path) | needs T-10 Type::Any; revisit post-v1.0 |
| Function statement hoisting (nested `function` inside blocks) | post-v1.0 polish | not started |
| Decorators / Mapped & conditional types / `Proxy` / JSX | out of scope | see `docs/roadmap.md` Out-of-scope features |

## Differences from bun (intentional)

- **AOT to native binary**: `tr build` produces a small statically-
  linked binary; bun bundles its V8 runtime.
- **Cold-start time**: torajs starts in ~1.2 ms; bun in ~7–8 ms;
  node-v8 in ~80 ms.
- **Bench scoreboard**: `tr build` wins on all 21 committed bench
  cases vs bun-aot (geomean 0.245x); vs rust geomean 0.656x — tr is
  ~34% faster than rust on average. See `docs/perf.md`.
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
