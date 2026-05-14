# torajs roadmap

> **v4 — test262 100% trunk.** Full rewrite on 2026-05-14 (HEAD `52ba8ea`,
> curated conformance 522/0/1, test262 in-scope 4 %). Replaces the prior v1
> (P0-P13 foundation), v2 (33-item perf-gated), and v3 (V3-XX wedge-cycle)
> plans. Those are preserved verbatim in `docs/roadmap-historical.md` —
> read them for *why* tora's foundation looks the way it does, not for
> *what to do next*.
>
> **HARD RULE for execution.** This file is the only forward-looking plan.
> Phases run strictly in order. Within a phase, items run strictly in order.
> No "candidate A vs B" branching at execution time — the order is the
> decision. Stop and discuss only on (a) genuine forks not in this doc,
> (b) irreversible decisions, (c) ambiguous-recovery failures.
>
> Living document — append observations / sub-items as they surface, but
> never reorder shipped items.

---

## Foundation

### Goal

Build a TypeScript runtime that runs the same TS programs `bun` runs, with
**TS semantics** — same observable behaviour as `bun` on the same source.
Anything bun runs, tr eventually runs; not-yet-implemented features are
roadmap phases, never out-of-scope decisions. The differentiator is the
runtime: AOT-compiled to a small native binary via LLVM (one path serves
both `tr build` and `tr run` — the latter caches the binary at
`~/.torajs/cache` for instant rerun), with ARC under a universal heap
header instead of GC. When behaviour is ambiguous, **bun is the oracle**.

### Hard requirements (unchanged from v1)

1. **极致 perf** — beat bun/node on important benchmarks; hold them.
2. **Compile not too slow** — first `tr run foo.ts` pays one full LLVM
   compile (~50–90 ms); subsequent runs hit the cache.
3. **Interpretable** — `tr run foo.ts` is the dev-loop entry point.
4. **No GC, internal ARC** for shared-heap values via a universal heap
   header. Single-owner uses compile-time ownership inference.
5. **TS-shape semantics** — what works, works the same as bun. No
   Rust-flavoured idioms in user code.
6. **Full TS coverage as a roadmap target** — every TS feature bun
   supports has a roadmap phase. Compile errors point at the phase.
7. **test262 ≥ 90 % on the in-scope slice as the v1.0 hard gate**;
   100 % is the stretch target driving this v4 trunk.

### Two-tier execution model (introduced 2026-05-14)

The single biggest insight from the wedge cycle (v3, V3-XX → 522 /521
curated, 4 % test262 in-scope): **test262 is 100 % JS source with no
type annotations, and tora's strict typecheck rejects most of it at the
first `var x = "anything"`.** Continuing to ship per-method wedges
plateaus against curated; it cannot move test262.

The v4 trunk fixes this with a two-tier model:

| Tier | When it applies | Layout / lower | Perf cost |
|---|---|---|---:|
| **typed-tier** | Source has explicit annotations (`x: number`) or the inference is concrete | Static layout, monomorphic ops, existing tora pipeline | 0 % regression |
| **untyped-tier** | Source is bare JS (`var x = expr`) where inference can only conclude `any` | 16-byte tagged-value slot `{tag: u8, payload: u64}`, runtime dispatch on tag | tagged-dispatch overhead, still AOT — no JIT, no interpreter |

**Performance-first invariant**: typed-tier code MUST NOT regress when the
untyped-tier lands. Every existing bench case stays in typed-tier (the
inference returns concrete types from `: T` annotations and from
unification). Adding untyped-tier is purely additive.

**Architecture-clean invariant**: Type::Any is a first-class type at
every layer (parser AST, check, ssa-lower, codegen). It is not a patch
on `__nullable` or a special case in BinOp — every op gets an Any-aware
arm. This makes the untyped-tier a clean parallel rail, not an escape
hatch.

---

## Status snapshot (2026-05-14, HEAD `52ba8ea`)

### Curated conformance (`conformance/cases/`)
**522 pass / 0 fail / 1 skip**. Effectively saturated by the v3 wedge
cycle. Remaining 1 skip is `perf-005-dwarf-panic-fs` (bun-side crash,
not tora's bug).

### test262 (200-sample, oracle = bun)
| Bucket | Count | Notes |
|---|---:|---|
| pass | 4 | tora matches bun byte-for-byte |
| bug | 0 | tora-accepted parity 100 % |
| incompatible (subset boundary) | 97 | 89 type error / 7 not yet supported / 1 parse error |
| bun-skip (oracle non-zero) | 99 | Negative tests / harness-dependent — never countable for tora |
| **in-scope pass rate** | **3.96 %** | 4 / 101 |
| **tr-accepted parity** | **100 %** | 4 / 4 |

### Bench position
Hold the v3 numbers: tr beats bun-jsc / bun-aot / node-v8 on
compute-heavy workloads (popcount, fib40, ...) — these all live in
typed-tier and stay locked there under v4.

### Code size
Three crates (`crates/torajs-{runtime,core,cli}/`), single SSA → LLVM
pipeline, no JIT, no interpreter.

---

## Trunk

The trunk is **15 phases (P-PARSE → P14) executed in strict order**.
Each phase has a measurable goal, an ordered item list, and an
acceptance gate (usually a test262 in-scope pass-rate target). The
order is fixed by substrate dependency — earlier phases unlock later
phases' work.

Phase budgets are rough (1 item ≈ 1–3 days, 1 phase ≈ weeks). They are
*planning estimates*, not commitments. The acceptance gate is the
contract.

### P-PARSE — ES syntax parser completeness

**Inserted 2026-05-14 after the first P0.1 commit revealed the
empirical bottleneck.** A 500-case `language/expressions` sample showed
parse errors at **53 % of all incompatibles** (159 / 300) — bigger
than type errors (135). The original P0 ("untyped-JS surface")
operates on the typecheck layer, but typecheck never runs when parse
already rejects. P-PARSE clears the parser surface first; only after
that do P0's typecheck-level changes get traction.

**Goal**: tora's parser accepts the full ES2024 expression / statement
/ pattern surface that test262 exercises in `language/expressions`,
`language/statements`, and `language/expressions/arrow-function/dstr`.

**Acceptance**: re-running the 500-case `language/expressions` sample,
`parse error` row in incompat breakdown drops from 159 to ≤ 20.

**Items (strict order — by empirical frequency in the 500-sample):**

- **P-PARSE.1** Sparse array literal `[1, , 3]` — comma in element
  position parses as elision (Empty), not error. Same shape inside
  destructuring patterns.
- **P-PARSE.2** Arrow-function parameter destructuring with nested
  array / object patterns — `([a, [b]]) => ...`,
  `([{ x }]) => ...`, `([...rest]) => ...`.
- **P-PARSE.3** Destructuring element with default value —
  `[a = 5] = ...`, `({ x = 1 }) = ...` (in destr-target position
  inside arrow / fn params).
- **P-PARSE.4** Object literal getter / setter shorthand —
  `{ get x() { ... }, set x(v) { ... } }`.
- **P-PARSE.5** Generator function expression `function* g() {...}`
  in expression position (the existing parser handles
  `function* g()` declaration but bails on `var g = function*() {...}`
  and similar shapes).
- **P-PARSE.6** Surface-residue items surfaced during P-PARSE.1–5
  (appended here as they're discovered, in order).

P-PARSE is intentionally *only* parser work — no SSA-lower changes,
no runtime additions. Each item is a token-aware parser branch, plus
the AST node it lowers to (which already exists for nearly all of
them since the typecheck path was already wired).

### P-COERCE-B — ToPrimitive in `+` for Struct / Function (deferred)

**Originally inserted 2026-05-14 as a B+C detour, deferred same day
after substrate audit.** ToPrimitive coerce in `+` requires the
ability to call a method on an object-literal value (e.g. emit
`o.valueOf() + n`). The probe `let o = { valueOf(): number {...} };
o.valueOf()` already fails today with an LLVM IR type mismatch
(PointerValue vs IntValue) — the object-literal-method-call substrate
isn't in place. Class instances with valueOf hit a different problem:
methods don't surface through struct field lookup, so the check.rs
detection couldn't even fire.

ToPrimitive coerce becomes tractable AFTER:
* Object-literal method dispatch is fixed (separate substrate, P3
  property-bag area).
* Class-instance method discovery surfaces in the type system (Type::
  Class augmentation, P3 / P7 area).

Not on the v4 trunk's critical path; will revisit when P3 / P7 lands.

### P-CLOSURE-C — closure monomorphization at call sites (deferred)

**Originally inserted 2026-05-14 alongside P-COERCE-B, deferred
same day after substrate audit.** Probed by lifting the closure-lift
skip in desugar_implicit_generics and accepting TypeVar-param at
call sites: typecheck went through but ssa_lower bailed with
'unknown ident `__closure_0`' because the closure FnDecl carries
TypeVars in its SSA signature with no monomorphization path. The
indirect-call mono retarget (looking up a let-bound closure's
source FnDecl and emitting a per-call-site specialization) is a
substantial substrate item that overlaps significantly with
P0's tagged-Any work.

Closure mono becomes tractable AFTER:
* Indirect-call mono path is wired into ssa_lower's monomorphizer
  (mirror of the bare-Ident global-FnDecl path), OR
* Tagged-Any closures (P0) — the closure body operates on Any
  operands so no per-call-site specialization is needed.

P0's tagged-Any path subsumes both. Going straight to P0 instead
of standing up a parallel mono substrate.

### P0 — Untyped-JS surface (original plan, resumes after detours)

**Goal**: tr accepts arbitrary unannotated `.js` source through typecheck.
Type::Any becomes a first-class participant in every operation; bare
`var x = expr` infers Any with runtime tag dispatch, while annotated
code stays in typed-tier with zero perf regression.

**Acceptance**: test262 in-scope pass rate ≥ 35 % (200-sample).
typed-tier benches lose 0 % vs HEAD.

**Items (strict order):**

- **P0.1** Type::Any tagged-value SSA representation
  - Add `Type::Any` first-class to `ssa.rs` (already exists nominally;
    promote to canonical 16-byte tagged-value layout)
  - Tag enum: `{ I64=0, F64=1, Bool=2, Null=3, Undefined=4, Str=5,
    Obj=6, Arr=7, Closure=8, BigInt=9, Symbol=10 }`
  - Slot layout: `[tag: u8 + 7-byte pad][payload: u64]` (16 B)
  - Codegen: `box_to_any(value, tag)` / `unbox_any(slot, expected_tag)`
    helper instructions
  - Refcount integration: tagged-Any carries Drop responsibility per
    payload tag — `unbox` on a refcounted tag inc's the parent slot
- **P0.2** Implicit `any` for unannotated bindings
  - check.rs: `let x = expr` with no annotation runs inference; if
    inference yields a concrete type → typed-tier; if `Any` (multi-arm
    BinOp result, runtime-only value) → untyped-tier
  - ssa_lower: when a binding is Any, its slot is the tagged-value
    layout from P0.1
  - Migration: existing typed-tier bindings keep their concrete types
    (no behaviour change); the new untyped path only fires when
    inference could not converge
- **P0.3** Any-aware BinOp / UnaryOp / Compare
  - `+`: Number + Number → Number; String + Any → coerce + concat;
    Any + Any → spec §13.15 ApplyStringOrNumericBinaryOperator
  - `-` / `*` / `/` / `%` / `**`: ToNumber both sides
  - `===` / `!==`: tag-compare first, then payload-compare
  - `==` / `!=`: spec §7.2.15 IsLooselyEqual
  - `<` / `<=` / `>` / `>=`: spec §7.2.13 IsLessThan
  - `!` / `+x` / `-x` / `~x` / `typeof`: tag-dispatch
- **P0.4** Member / Index access on Any
  - `any.prop`: dispatch by tag: Obj → property bag (P3),
    Arr → length / index, Str → length / charAt-style, ...
  - `any[expr]`: same dispatch, with int vs string key resolution
  - The full property-bag substrate lands in P3; P0.4 is the
    placeholder Any → typed-shape bridge (when the inference can
    pin a tag, it routes to the existing typed path)
- **P0.5** Call on Any
  - `any(...args)`: tag must be Closure / FnSig at runtime, else throw
    TypeError (real Error lands in P6; for now: panic with a
    spec-shaped message)
  - Args are tagged-Any, return is tagged-Any
- **P0.6** typeof on Any
  - Tag → string per spec §13.5.3 (Number → "number", String →
    "string", Boolean → "boolean", Null → "object", Undefined →
    "undefined", Closure → "function", others → "object")
- **P0.7** ToBoolean / ToNumber / ToString on Any
  - Spec §7.1.2 / §7.1.4 / §7.1.17 algorithms
  - Used by every coercion site (`if (any)`, `+any`, `String(any)`)
- **P0.8** test262 runner integration
  - `--bucket-by-tier` flag: separately report typed-tier-only pass
    rate, untyped-tier pass rate, mixed
  - Add per-incompat reason tracking (already exists; extend with
    "implicit-any-not-supported" → expect to vanish in P0)
  - Re-run 200-sample; verify ≥ 35 % in-scope pass

### P1 — `undefined` as a real value

**Goal**: `undefined` is a distinct value from `null` end-to-end. Default
parameters that aren't passed get `undefined`; OOB array reads return
`undefined`; `typeof undefined === "undefined"`.

**Acceptance**: test262 in-scope pass rate ≥ 45 %.

**Items (strict order):**

- **P1.1** Type::Undefined first-class in `ssa.rs` and `check.rs`
- **P1.2** Tag value for Undefined slot in P0.1 layout (already
  reserved as tag=4)
- **P1.3** Default parameter missing → undefined (currently → null)
- **P1.4** Array.find / .at / .indexOf etc. OOB → undefined
  (currently → 0 / -1 / etc. depending on method)
- **P1.5** typeof undefined → "undefined" (currently "object" via
  Null)
- **P1.6** Optional chain: `x?.y` where `x` is undefined → undefined
- **P1.7** `Nullable<T>` becomes `T | null | undefined` per spec; the
  existing `__nullable(T)` ann stays as a synonym for the
  `T | null | undefined` shape
- **P1.8** Strict equality: `undefined === null` → false (currently
  true via collapse)

### P2 — `var` and function hoisting

**Goal**: `var x` and `function f` follow ES §14.1.3 hoisting rules.
TDZ for `let` / `const` stays as-is.

**Acceptance**: test262 in-scope pass rate ≥ 50 %.

**Items (strict order):**

- **P2.1** Two-pass scope analysis: collect var / function decls before
  body lower
- **P2.2** `var x` hoists to enclosing function scope (not block)
- **P2.3** `function f() {}` hoists declaration + binding to scope-top
- **P2.4** `for (var i ...) { ... } ; use(i)` — i leaks
- **P2.5** Hoisted-but-not-yet-assigned var reads as `undefined` (P1.3
  pattern)

### P3 — Property-bag objects

**Goal**: objects support runtime add / delete / computed keys. Static
shape inference picks dict-shape vs struct-shape per binding so existing
typed code stays on static layout.

**Acceptance**: test262 in-scope pass rate ≥ 60 %. Typed-tier perf 0 %
regression.

**Items (strict order):**

- **P3.1** Dict-shape object layout: 32-byte header + open-addressed
  hash table {hash, key_str, value_tagged}
- **P3.2** Inference: a binding stays static-struct if all member
  access sites use compile-time-known field names AND no `delete` /
  `Object.assign` / spread-into-it; otherwise dict-shape
- **P3.3** `obj.newProp = v` adds property at runtime (dict-shape only)
- **P3.4** `delete obj.x` removes property (dict-shape only)
- **P3.5** `Object.keys / values / entries` reads dict
- **P3.6** Computed property keys: `{ [k]: v }`
- **P3.7** Symbol keys (interns symbol id into the hash key space)
- **P3.8** Object.freeze / isFrozen with the universal heap header's
  flag bit (deferred from v0.2)

### P4 — Iterator protocol

**Goal**: `Symbol.iterator` is a real resolvable property; for-of
dispatches via it; spread-in-call works.

**Acceptance**: test262 in-scope pass rate ≥ 65 %.

**Items (strict order):**

- **P4.1** Iterator result `{ value, done }` shape standardised in
  runtime
- **P4.2** Symbol.iterator as a registered well-known symbol; user
  classes can implement it
- **P4.3** for-of dispatches via [Symbol.iterator]() on any value
  (current path is hard-wired to Array / Str / Set)
- **P4.4** arr.entries() / .keys() / .values() return Array Iterator
  objects
- **P4.5** Spread in fn calls: `f(...iter)` (currently parse-rejected)
- **P4.6** Spread in array literal: `[...iter, x]` over any iterable

### P5 — Map / Set / WeakMap / WeakSet

**Goal**: real hash containers, all spec methods.

**Acceptance**: test262 in-scope pass rate ≥ 70 %.

**Items (strict order):**

- **P5.1** Map<K, V> hash table runtime (open-addressed, robin hood)
- **P5.2** Set<T> = Map<T, undefined> wrapper
- **P5.3** WeakMap / WeakSet with weak-ref tracker bits
- **P5.4** Spec methods: get / set / delete / has / clear / size /
  forEach / entries / keys / values
- **P5.5** Iterator interop with P4

### P6 — Error type hierarchy + throw any

**Goal**: real Error subtypes (TypeError, RangeError, SyntaxError, ...);
`throw` accepts any value; try/catch/finally state machine spec-conformant.

**Acceptance**: test262 in-scope pass rate ≥ 75 %.

**Items (strict order):**

- **P6.1** Error class + subclass hierarchy in stdlib
- **P6.2** `throw <any value>` (currently restricted to Str / a few
  shapes)
- **P6.3** Stack trace captured at throw site (uses DWARF data)
- **P6.4** Native errors: runtime helpers throw real RangeError /
  TypeError where spec says (e.g. toFixed(101), null.x)
- **P6.5** try / catch / finally state-machine matches spec ordering

### P7 — Class spec full

**Goal**: private fields, static blocks, accessor properties,
super-in-arrow.

**Acceptance**: test262 in-scope pass rate ≥ 78 %.

**Items (strict order):**

- **P7.1** `#priv` private fields (parser + lower with name mangling)
- **P7.2** Class getters / setters (accessor descriptors)
- **P7.3** Static blocks `static { ... }`
- **P7.4** Lexical super resolution in nested arrows
- **P7.5** Class expressions as values

### P8 — Regex full

**Goal**: spec-complete RegExp incl. lookahead / lookbehind, named
groups, Unicode flag, sticky flag.

**Acceptance**: test262 in-scope pass rate ≥ 81 %.

**Items (strict order):**

- **P8.1** Lookbehind / lookahead (current NFA needs backtracking
  extension)
- **P8.2** Named capture groups + back-references
- **P8.3** Unicode flag (`u` / `v`) — character class handling
- **P8.4** Sticky flag (`y`) — lastIndex semantics
- **P8.5** `String.prototype.replace(regex, fn)` callback form

### P9 — Promise + async-await spec

**Goal**: real microtask queue, ordering guarantees, async iterators.

**Acceptance**: test262 in-scope pass rate ≥ 84 %.

**Items (strict order):**

- **P9.1** Microtask queue with drain at every yield point
- **P9.2** Promise.all / .race / .allSettled / .any per spec
  (currently allSettled is single-T MVP)
- **P9.3** Async iterator + for-await-of
- **P9.4** await on non-Promise: wrap via Promise.resolve
- **P9.5** unhandledrejection handler hook

### P10 — String Unicode

**Goal**: UTF-16 internal representation, codepoint iteration, full
Unicode case folding.

**Acceptance**: test262 in-scope pass rate ≥ 87 %.

**Items (strict order):**

- **P10.1** Convert byte-Str runtime to UTF-16 internal (or hybrid
  Latin-1 / UTF-16 like V8)
- **P10.2** String.length = UTF-16 code unit count
- **P10.3** charCodeAt vs codePointAt distinction (surrogate pairs)
- **P10.4** for-of on string yields codepoints (with surrogate
  combining)
- **P10.5** Full Unicode case folding (lowercase / uppercase per
  CaseFolding.txt)
- **P10.6** String.normalize NFC / NFD / NFKC / NFKD via libicu (or
  embedded data tables)

### P11 — Number IEEE 754 conformance

**Goal**: Number.toString / parseFloat / arithmetic match spec exactly,
incl. the long-tail rounding cases.

**Acceptance**: test262 in-scope pass rate ≥ 89 %.

**Items (strict order):**

- **P11.1** Number::toString full §6.1.6.1.13 algorithm (Steele-White
  / Ryu — replace `%g` precision-loop)
- **P11.2** parseFloat / parseInt edge cases (whitespace per spec
  table, prefix detection)
- **P11.3** IEEE rounding modes for toFixed / toPrecision
  (round-half-to-even vs away-from-zero)
- **P11.4** BigInt full operator coverage incl. `**`, mixed-shift,
  spec-conformant overflow

### P12 — Module system

**Goal**: ESM static analysis, dynamic `import()`, top-level await.

**Acceptance**: test262 in-scope pass rate ≥ 90 % (v1.0 hard gate).

**Items (strict order):**

- **P12.1** Static import / export resolution at compile time
- **P12.2** Dynamic `import()` returning Promise (link to P9
  microtask queue)
- **P12.3** Module-level top-level await
- **P12.4** Module namespace object (`import * as X`)

### v1.0 release gate

**P0–P12 done = v1.0**. test262 in-scope pass rate ≥ 90 % is the
contract. Cut release tag. Bench numbers must show ≤ 5 % regression
on typed-tier vs HEAD (untyped-tier has no bench gate — it's correctness
work).

### P13 — Proxy + Reflect (post-v1.0)

**Goal**: meta-object protocol, all 13 trap types.

**Acceptance**: test262 in-scope pass rate ≥ 94 %.

**Items (strict order):**

- **P13.1** Proxy class with handler trap dispatch
- **P13.2** Reflect.* spec methods (Reflect.get / set / has / ...)
- **P13.3** Trap interop with Object.keys / for-in / etc.
- **P13.4** Proxy.revocable

### P14 — Generator full + tail call (post-v1.0)

**Goal**: yield* delegation, generator return / throw protocol, proper
tail calls in strict mode.

**Acceptance**: test262 in-scope pass rate ≥ 96 %.

**Items (strict order):**

- **P14.1** `yield*` delegation
- **P14.2** Generator.prototype.return / .throw
- **P14.3** Tail call optimisation in strict mode (per spec §15.10.3)

### Beyond P14 — long tail to 100 %

Last 4 % is test262's edge: annexB legacy semantics, locale-dependent
behaviour, host hook tests. Hit only after P0–P14 ship and the runner
breakdown points specifically here. **Not pre-planned** — reach this
chapter and we open a new sub-trunk.

---

## Execution rules

1. **Phase order is fixed.** Do not start P(N+1) until P(N)'s acceptance
   gate is met.
2. **Item order within a phase is fixed.** Each item's commit message
   names the item id (e.g. `P0.3`).
3. **Every commit ships through the conformance gate.** `conf gate`
   must stay green. test262 in-scope rate must not drop.
4. **Typed-tier benches gate every commit.** No bench regression past
   3 × CI noise.
5. **Stop and discuss only on:**
   - design forks not in this doc (e.g. dict-shape hash policy choice)
   - irreversible decisions (e.g. dropping a feature from a phase)
   - ambiguous-recovery failures (e.g. a substrate item turns out to
     need its own substrate)
6. **Do not branch out of this doc** to side cleanups, refactors, or
   nice-to-have wedges. Append them as P{N}.{x+1} if they're needed
   for the current phase, or `## Backlog` (below) if they're not.

---

## Backlog (orthogonal items, not on the trunk)

These are useful but not on the test262-100% critical path. Pick them up
between phases only when blocked, never as a primary track.

- **f64.toString(radix) trailing-digit round-half-to-even** — current
  helper truncates at 52 digits; bun rounds the 53rd. Affects
  long-fraction cases only.
- **Array<f64> literal layout** — `let xs: number[] = [1.5, 2.5]`
  currently stores f64 bits in i64 slots; need real f64 array
  layout.
- **SameValueZero NaN-in-Array<f64>.includes** — `includes` on
  Array<f64> for NaN should return true; FCmp(Oeq) returns false.
  Needs a dedicated NaN-self-test in the includes-only path.
- **String.search with RegExp arg** — currently string-arg only.
  Symbol.search dispatch is the wider substrate.
- **`xs.length = N` array truncation** — write-side on `.length`.
- **Closure parameter type inference from outer call site** —
  filter/map/reduce on Struct array currently rejects.
- **Generic explicit type-arg `id<T>(x)`** — substrate via the
  generic-instantiation path (probably part of P0).
- **Generator multi-class alias bug** — see notes on
  parser-generator-wrapped-return-001 fixture.
- **`this` on static class method** — currently `unknown identifier
  __this`.
- **JSON.stringify with indent (2nd arg)** — currently the indent
  arg is ignored.
- **`typeof x === "type"` narrow** — narrows aren't yet aware of
  `typeof` shape; covered partially by P3 / P0.

---

## Principles (kept from v1)

- Foundation: `docs/design-principles.md` — five-pillar rubric (高性能
  / 自研 / 正统 / 规范 / 上限优先).
- Refcount + universal heap header: `docs/refcount-architecture.md`.
- Coding rules: `.claude/rules/common/`, `.claude/rules/{rust,typescript}/`.

---

## BENCH — cross-runtime perf benchmark (cross-cutting track)

Runs on every commit alongside conformance. Same set of cases as
v3 (popcount, fib40, ...). Acceptance: typed-tier benches stay green
end-to-end through P0–P14.

The detailed bench harness layout, oracle setup, and per-case budget
table live in `docs/bench.md` (TODO: extract from v3 roadmap appendix).

---

## Historical roadmaps

`docs/roadmap-historical.md` preserves the v1 (P0–P13 foundation), v2
(33-item perf-gated), and v3 (V3-XX wedge-cycle to 522/521 curated)
plans verbatim. Read them for the *why* of tora's foundation. Do not
read them for *what to do next* — that lives only in this file.
