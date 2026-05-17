# torajs roadmap

> **v5 — 三轴并行 trunk.** Rewritten 2026-05-17 (HEAD `a65e51f`, curated
> conformance 590/0/1, 5k diagnostic 152/15/2975). Supersedes v4
> (test262-100% trunk, 2026-05-14). The v4 trunk treated test262
> in-scope pass rate as the per-phase acceptance metric; v5 replaces
> that with **substrate-checklist acceptance** (concrete spec sections
> worked, runtime + ssa-lower paths landed). Pass rate stays as a
> diagnostic / regression detector — never a milestone.
>
> Prior trunks (v1 P0-P13 foundation, v2 33-item perf-gated, v3 V3-XX
> wedge cycle, v4 test262-100%) are preserved verbatim in
> `docs/roadmap-historical.md`. Read them for *why* tora's foundation
> looks the way it does, not for *what to do next*.
>
> **HARD RULE for execution.** This file is the only forward-looking
> plan. Phases run strictly in order. Within a phase, items run
> strictly in order. No "candidate A vs B" branching at execution
> time — the order is the decision. Stop and discuss only on (a)
> genuine forks not in this doc, (b) irreversible decisions, (c)
> ambiguous-recovery failures.
>
> Living document — append observations / sub-items as they surface,
> but never reorder shipped items.

---

## Foundation

### Goal — three axes

torajs 是 AOT 编译型 TypeScript runtime，差异化是 native binary + 小
artifact + fast startup。Long-arc 终态由三轴定义，**三轴并行推进，不接受
为某一轴妥协另一轴**。每 phase 同时推三轴；任一轴失败 = phase 不收口。

**轴 A — spec completeness（正统）**

终态：test262 全量 100% pass over in-scope（不是 5k sample，不是 90%
gate）。每 phase 的 acceptance 用 **spec-section checklist**（具体 spec
章节的硬事实，"§7.1.3 ToNumber via valueOf works on Struct" 这种粒度）
验收。pass rate 数字是 diagnostic / regression detector，不作 milestone。

**轴 B — performance ceiling（高性能 + 上限优先）**

终态：在 bench-tr 套件 cross-runtime 对比上 SOTA — 每个 case 严格优于
bun-aot / bun-jsc / nodejs / go / rust 各自对位。perf push 跟 spec 推进
**并行**（不是 v1.0 完了再优化）：每 phase 内部既推 spec 又拉 bench。
**bench-tr 0 regression 是每 commit 的硬阈值**；每 N phase 做一次
perf-focused push 拉新 case 进 SOTA 范围。

**轴 C — implementation purity（自研）**

终态：runtime + 编译器内核全自研。不嵌入 V8 / JSC / QuickJS。允许的
外部依赖：

- build-time 工具：LLVM / inkwell / cranelift（last-stable，pin 到具体
  minor）
- runtime-side 系统接口：libc 唯一
- Rust host crates（serde / tokio / 等）仅用在 host 编译期，不进
  runtime binary
- **每 phase ship 时不允许引入新非高品质依赖**。"我能找到一个 crate 做
  这事" 不是引入它的理由 — 必须 audit (a) crate 质量 (b) 是否能自研
  替代 (c) last-stable 锁版本。

三轴的硬冲突时：质量优先（轴 A 正确性）> 性能优先（轴 B）> 自研优先
（轴 C）。极少出现冲突；通常三轴同向。

### Hard requirements (kept from v1)

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
7. **test262 in-scope 100%** as the v1.0 stretch target — gated by
   substrate completeness, not by pass-rate %.

### Two-tier execution model (introduced 2026-05-14, kept)

The single biggest insight from the wedge cycle (v3, V3-XX → 522/521
curated, 4 % test262 in-scope): **test262 is 100 % JS source with no
type annotations, and tora's strict typecheck rejects most of it at
the first `var x = "anything"`.** Continuing to ship per-method wedges
plateaus against curated; it cannot move test262.

The v4 trunk fixed this with a two-tier model; v5 keeps it.

| Tier | When it applies | Layout / lower | Perf cost |
|---|---|---|---:|
| **typed-tier** | Source has explicit annotations (`x: number`) or the inference is concrete | Static layout, monomorphic ops, existing tora pipeline | 0 % regression |
| **untyped-tier** | Source is bare JS (`var x = expr`) where inference can only conclude `any` | 16-byte tagged-value slot `{tag: u8, payload: u64}`, runtime dispatch on tag | tagged-dispatch overhead, still AOT — no JIT, no interpreter |

**Performance-first invariant**: typed-tier code MUST NOT regress when
the untyped-tier lands. Every existing bench case stays in typed-tier
(the inference returns concrete types from `: T` annotations and from
unification). Adding untyped-tier is purely additive.

**Architecture-clean invariant**: Type::Any is a first-class type at
every layer (parser AST, check, ssa-lower, codegen). It is not a patch
on `__nullable` or a special case in BinOp — every op gets an Any-aware
arm. This makes the untyped-tier a clean parallel rail, not an escape
hatch.

---

## Status snapshot (2026-05-17, HEAD `a65e51f`)

### Curated conformance (`conformance/cases/`)

**590 pass / 0 fail / 1 skip** committed. Working tree has +1 RED
fixture `check-prototype-chain-001.ts` waiting for P4.2 ship to turn
GREEN. The 1 committed skip is `perf-005-dwarf-panic-fs` (bun-side
crash, not tora's bug).

### test262 5k diagnostic

**152 pass / 15 bug / 2975 incompatible** at HEAD. Stable across the
last 4 P3 substrate ships (`749c1d4` → `dcf069f` → `d9b13c7` →
`a65e51f`). **Pass rate is regression-detection only — not a phase
trigger or milestone.**

### Bench position

Typed-tier 0 regression invariant holds. Latest cross-runtime bench
(at `e19cac3` / `008cd84`): torajs vs bun-aot geomean **4.16×**, vs
node-v8 geomean **18.84×**, binary size **1715× smaller** than bun-aot.

### Code size

Three crates (`crates/torajs-{runtime,core,cli}/`), single SSA → LLVM
pipeline, no JIT, no interpreter.

---

## Trunk

The trunk is **P0 → P13 (v1.0 gate) + P14 / P15 post-v1.0**, executed
in strict order. Phase order is fixed by substrate dependency — earlier
phases unlock later phases' work.

**Per-phase acceptance has three parts (all required):**

1. **Substrate checklist** — concrete spec sections / ssa-lower paths /
   runtime helpers landed. Phase-specific, listed below.
2. **Bench gate** — bench-tr cross-runtime suite shows 0 regression
   vs phase-start baseline. Untyped-tier additions don't gate; they
   are correctness work.
3. **自研 audit** — no new external dependencies introduced beyond the
   foundation set (libc / LLVM / inkwell / cranelift). Any addition
   requires explicit justification + last-stable pinning.

Phase budgets are rough (1 item ≈ 1–3 days, 1 phase ≈ weeks). Planning
estimates only; the **substrate checklist is the contract**.

---

### P0 — Untyped-JS surface (DONE substantial)

**Goal**: tr accepts arbitrary unannotated `.js` source through
typecheck. Type::Any is a first-class participant in every operation.

**Substrate checklist** (closed):

- [x] **P0.1** Type::Any tagged-value SSA representation (16-byte
      `{tag: u8 + 7 pad, payload: u64}`)
- [x] **P0.2** Implicit `any` for unannotated bindings
- [x] **P0.3** Any-aware BinOp / UnaryOp / Compare (`+` / `-` / `*` /
      `/` / `%` / `**` / `===` / `==` / `<` / etc.)
- [x] **P0.4** Member / Index access on Any (placeholder Any →
      typed-shape bridge; full property-bag in P3)
- [x] **P0.5** Call on Any (Closure / FnSig tag dispatch)
- [x] **P0.6** typeof on Any (spec §13.5.3)
- [x] **P0.7** ToBoolean / ToNumber / ToString on Any (spec §7.1.2 /
      §7.1.4 / §7.1.17)
- [x] **P0.8** test262 runner --bucket-by-tier flag + per-incompat
      reason tracking

**Bench**: typed-tier 0 regression ✅
**自研**: 无新依赖 ✅

---

### P1 — undefined as a real value (DONE)

**Goal**: `undefined` is a distinct value from `null` end-to-end.

**Substrate checklist** (closed):

- [x] **P1.1** Type::Undefined first-class in ssa.rs / check.rs
- [x] **P1.2** Tag value for Undefined slot
- [x] **P1.3** Default parameter missing → undefined
- [x] **P1.4** Array.find / .at / .indexOf OOB → undefined
- [x] **P1.5** typeof undefined → "undefined"
- [x] **P1.7** `Nullable<T>` = `T | null | undefined`
- [x] **P1.8** `undefined === null` → false

**Bench / 自研**: ✅ / ✅

---

### P2 — var/function hoisting (DONE substantial)

**Goal**: ES §14.1.3 hoisting rules. TDZ for `let` / `const` stays.

**Substrate checklist** (closed):

- [x] **P2.1** Two-pass scope analysis + var/function hoist
- [x] **P2.4** for-var leak (`for (var i ...) { ... }; use(i)`)
- [x] Module-top block fn lift (P3.4-followup-A)
- [x] Bare FnDecl-as-stmt (P3.4-followup-A2)

**Bench / 自研**: ✅ / ✅

---

### P3 — Property-bag objects (DONE close, `d9b13c7`)

**Goal**: objects support runtime add / delete / computed keys + full
property descriptor semantics. Static shape inference picks dict-shape
vs struct-shape so existing typed code stays on static layout.

**Substrate checklist** (closed):

- [x] **P3.1** SwissTable dict-shape runtime
- [x] **P3.2** Dynobj inference (struct vs dict)
- [x] **P3.3** defineProperty runtime + spec-shaped descriptor
- [x] **P3.4** Nested fn hoist + module-top block-fn lift
- [x] **P3.5** OptChain Any-tier (`a?.b?.c` 多层链)
- [x] **P3.struct-method-dispatch** — inline `obj.method()` for FnSig +
      Closure field (`1a308f7`)
- [x] **P3.closure-in-struct-field** — narrow Closure ABI via
      `__cls(...)->R` struct-field tagging (`749c1d4`)
- [x] **P3.attribute-flag-tracking** — bucket.tag 高位 packed flag bits
      + `__torajs_dynobj_define` 实施 spec §10.1.6.3 (`dcf069f`)
- [x] **P3.getOwnPropertyDescriptor** — `__torajs_get_property_descriptor`
      一步构造 spec-shaped descriptor Any-box (`d9b13c7`)
- [x] Object.keys / values / entries
- [x] Computed property keys `{ [k]: v }`
- [x] Symbol keys
- [x] Object.freeze (universal heap header flag bit)

**Bench / 自研**: ✅ / ✅

**P3 后续残项**（升 L3a 时按 substrate-correct 标准 ship）：

- T-42 ToNumber via valueOf (§7.1.3) — prerequisite to many spec paths
- P3.5 OptChain 链式 typed-dispatch
- P3.4 nested fn 真实 closure capture
- T-31-followup closure / FnSig 间接调 callee real_argc
- T-45-b `in` operator on Struct / Closure / FnSig / String

---

### P4 — Class hierarchies + prototype chain (CURRENT)

**Goal**: tora 的 nominal class system 升级到 spec §10.1
OrdinaryObject + §10.4 ExoticObject 的 `[[Prototype]]` / `[[Get]]` /
`[[Set]]` 内部 method 模型。class extends + super() + builtin extends
全部走 prototype chain。

**Substrate checklist** (strict order):

- [x] **P4.1 Phase A1** First-class class objects (SHIPPED `a65e51f`)
      — `synthesize_class_globals` desugar pass; `const x = MyClass`
      resolves to dynobj-backed Any
- [ ] **P4.0** Nested Any-dynobj field identity fix (pre-blocker for
      Phase B+C) — `outer.p === inner` when inner is Any-typed dynobj;
      ssa_lower `dynobj_init` Type::Any field path (line 11443+) +
      Member read (line 21522+) + box_to_any path (line 11487+)
- [ ] **P4.2 Phase B+C** Prototype singletons + chain wiring +
      `Object.getPrototypeOf` / `setPrototypeOf` real readback
      (depends on P4.0)
- [ ] **P4.3 extends-chain** — multi-level inheritance method resolve
      via prototype chain walk; instance.method() walks chain
- [ ] **P4.4 function-prototype** — `Function.prototype.bind / call /
      apply` (spec §20.2.3.1-3); bind needs closure-style partial
      application + bound-this `[[Call]]`
- [ ] **P4.5 new-meta** — `new X.Y()` member-expr ctor + `new.target`
      meta-property in ctor body
- [ ] **P4.6 extends-builtins** — `class MyError extends Error` /
      `class MyArray extends Array` ; builtin types expose prototype
      objects + extends 链能链到它们
- [ ] **P4.7 catch-destructure** — `try {} catch ({code, msg}) {}` 真
      binding; parser already accepts but runtime ignores destructure
      pattern

**Acceptance**: 7 substrate items all 完成 + conformance 0 fail +
bench-tr 0 regression + 无新非高品质外部依赖。

**P4.0 详情**（next L3a，2-4h）：

```ts
// reproduce, both from user-shape, no class system involved
let inner: any = { x: 1 };
let outer: any = { p: inner };
console.log(outer.p === inner);  // bun true / tora false
console.log(outer.p.x);          // bun 1   / tora undefined
```

Possible root causes (to confirm via SSA IR trace):

1. `Load(I64, v_raw, 16)` v_raw 可能不是 Any-box ptr 而是 slot ptr
2. box_to_any_from_expr 路径在 dynobj init 时可能没真正发生
3. 实际 stored value 是 dynobj 的 i64 representation 不正确
4. dynobj_get_value 读出的值经过 truncation 或 mask 丢失某些位

**P4.2 设计** (待 P4.0 ship 后落地)：

- `let __proto_<C>: any = {}` 同义
- `__class_<C> = { prototype: __proto_<C>, name }`
- `__proto_<Sub>.__proto__ = __proto_<Super>` chain wire
- runtime helper `__torajs_get_proto_of_any` (Type::Any 路径)
- ssa_lower intercept (Type::Obj 路径 reverse-lookup sid→class_name→load
  `__proto_<C>` local)

K.3 globals 不扩 Type::Any（本会话 design 决定）；prototype singleton
存放选 **Option 2 runtime side table**（class-name 字符串 keyed），
bypass K.3 entirely。Long-term most robust + decoupled from K.3
design constraints。

---

### P5 — Iterator protocol

**Goal**: `Symbol.iterator` is a real resolvable property; for-of
dispatches via it; spread-in-call works for arbitrary iterables.

**Substrate checklist** (strict order):

- [ ] **P5.1** Iterator result `{ value, done }` shape standardised in
      runtime
- [ ] **P5.2** Symbol.iterator as registered well-known symbol; user
      classes can implement it
- [ ] **P5.3** for-of dispatches via `[Symbol.iterator]()` on any value
      (current path is hard-wired to Array / Str / Set)
- [ ] **P5.4** `arr.entries()` / `.keys()` / `.values()` return Array
      Iterator objects
- [ ] **P5.5** Spread in fn calls: `f(...iter)` (currently
      parse-rejected)
- [ ] **P5.6** Spread in array literal: `[...iter, x]` over any iterable

---

### P6 — Map / Set / WeakMap / WeakSet

**Goal**: real hash containers, all spec methods.

**Substrate checklist** (strict order):

- [ ] **P6.1** `Map<K, V>` hash table runtime (open-addressed, robin
      hood)
- [ ] **P6.2** `Set<T>` = `Map<T, undefined>` wrapper
- [ ] **P6.3** WeakMap / WeakSet with weak-ref tracker bits
- [ ] **P6.4** Spec methods: get / set / delete / has / clear / size /
      forEach / entries / keys / values
- [ ] **P6.5** Iterator interop with P5

---

### P7 — Error type hierarchy + throw any

**Goal**: real Error subtypes (TypeError, RangeError, SyntaxError, …);
`throw` accepts any value; try/catch/finally state machine
spec-conformant.

**Substrate checklist** (strict order):

- [ ] **P7.1** Error class + subclass hierarchy in stdlib (depends on
      P4 class hierarchy)
- [ ] **P7.2** `throw <any value>` (currently restricted to Str / a
      few shapes)
- [ ] **P7.3** Stack trace captured at throw site (uses DWARF data)
- [ ] **P7.4** Native errors: runtime helpers throw real RangeError /
      TypeError where spec says
- [ ] **P7.5** try / catch / finally state-machine matches spec
      ordering

---

### P8 — Class spec full (private + static blocks + accessor + super)

**Goal**: complete the class feature set started in P4 — private
fields, static blocks, accessor properties, super-in-arrow.

**Substrate checklist** (strict order):

- [ ] **P8.1** `#priv` private fields (parser + lower with name
      mangling)
- [ ] **P8.2** Class getters / setters (accessor descriptors)
- [ ] **P8.3** Static blocks `static { ... }`
- [ ] **P8.4** Lexical super resolution in nested arrows
- [ ] **P8.5** Class expressions as values

---

### P9 — Regex full

**Goal**: spec-complete RegExp incl. lookahead / lookbehind, named
groups, Unicode flag, sticky flag.

**Substrate checklist** (strict order):

- [ ] **P9.1** Lookbehind / lookahead (current NFA needs backtracking
      extension)
- [ ] **P9.2** Named capture groups + back-references
- [ ] **P9.3** Unicode flag (`u` / `v`) — character class handling
- [ ] **P9.4** Sticky flag (`y`) — lastIndex semantics
- [ ] **P9.5** `String.prototype.replace(regex, fn)` callback form

---

### P10 — Promise + async-await + Generator

**Goal**: real microtask queue, ordering guarantees, async iterators,
generator full state machine. v5 merges v4's P9 (Promise) + P14
(Generator) into one phase — both share state-machine substrate.

**Substrate checklist** (strict order):

- [ ] **P10.1** Microtask queue with drain at every yield point
- [ ] **P10.2** Promise.all / .race / .allSettled / .any per spec
      (currently allSettled is single-T MVP)
- [ ] **P10.3** Async iterator + for-await-of (depends on P5)
- [ ] **P10.4** await on non-Promise: wrap via Promise.resolve
- [ ] **P10.5** unhandledrejection handler hook
- [ ] **P10.6** Generator full state machine — `yield*` delegation +
      `Generator.prototype.return` / `.throw`
- [ ] **P10.7** Default-Any Generator/Async fn (T-33 substrate)

---

### P11 — String Unicode

**Goal**: UTF-16 internal representation, codepoint iteration, full
Unicode case folding.

**Substrate checklist** (strict order):

- [ ] **P11.1** Convert byte-Str runtime to UTF-16 internal (or hybrid
      Latin-1 / UTF-16 like V8)
- [ ] **P11.2** `String.length` = UTF-16 code unit count
- [ ] **P11.3** `charCodeAt` vs `codePointAt` distinction (surrogate
      pairs)
- [ ] **P11.4** for-of on string yields codepoints (with surrogate
      combining)
- [ ] **P11.5** Full Unicode case folding (lowercase / uppercase per
      CaseFolding.txt)
- [ ] **P11.6** `String.normalize` NFC / NFD / NFKC / NFKD (embedded
      data tables — no libicu unless 自研 audit passes)

---

### P12 — Number IEEE 754 conformance

**Goal**: Number.toString / parseFloat / arithmetic match spec
exactly, incl. the long-tail rounding cases.

**Substrate checklist** (strict order):

- [ ] **P12.1** `Number::toString` full §6.1.6.1.13 algorithm
      (Steele-White / Ryu — replace `%g` precision-loop)
- [ ] **P12.2** parseFloat / parseInt edge cases
- [ ] **P12.3** IEEE rounding modes for `toFixed` / `toPrecision`
- [ ] **P12.4** BigInt full operator coverage incl. `**`, mixed-shift,
      spec-conformant overflow

---

### P13 — Module system → v1.0 gate

**Goal**: ESM static analysis, dynamic `import()`, top-level await.

**Substrate checklist** (strict order):

- [ ] **P13.1** Static import / export resolution at compile time
- [ ] **P13.2** Dynamic `import()` returning Promise (links to P10
      microtask queue)
- [ ] **P13.3** Module-level top-level await
- [ ] **P13.4** Module namespace object (`import * as X`)

### v1.0 release gate

**P0–P13 substrate-checklists all closed = v1.0**. Per-phase acceptance
gates above are the contract — substrate sections done, conformance
gate green, bench-tr 0 regression on typed-tier, no new external
dependencies. test262 in-scope pass rate is observed (expected ≥ 90 %)
but not the gate; the gate is substrate completeness.

---

### P14 — Proxy + Reflect (post-v1.0)

**Goal**: meta-object protocol, all 13 trap types.

**Substrate checklist** (strict order):

- [ ] **P14.1** Proxy class with handler trap dispatch
- [ ] **P14.2** `Reflect.*` spec methods
- [ ] **P14.3** Trap interop with Object.keys / for-in / etc.
- [ ] **P14.4** Proxy.revocable

---

### P15 — Tail call + edge spec (post-v1.0)

**Goal**: proper tail calls in strict mode + remaining spec edges
(annexB legacy, locale-dependent behaviour, host hook tests).

**Substrate checklist** (strict order):

- [ ] **P15.1** Tail call optimisation in strict mode (spec §15.10.3)
- [ ] **P15.2** annexB legacy semantics
- [ ] **P15.3** locale-dependent behaviour (Intl subset)
- [ ] **P15.4** Host hook tests — open new sub-trunk when runner
      breakdown points here

---

## Execution rules

1. **Phase order is fixed.** Do not start P(N+1) until P(N)'s substrate
   checklist is closed.
2. **Item order within a phase is fixed.** Each item's commit message
   names the item id (e.g. `P4.0`).
3. **Every commit ships through the conformance gate.** `conf gate`
   must stay green.
4. **Typed-tier bench gates every commit.** No regression past 3× CI
   noise.
5. **Stop and discuss only on:**
   - design forks not in this doc (e.g. K.3 globals扩 Type::Any vs
     runtime side table choice)
   - irreversible decisions (e.g. dropping a feature from a phase)
   - ambiguous-recovery failures (e.g. a substrate item turns out to
     need its own substrate — log as a P{N}.0 pre-blocker)
6. **Do not branch out of this doc** to side cleanups, refactors, or
   nice-to-have wedges. Append them as P{N}.{x+1} if they're needed
   for the current phase, or to `## Backlog` (below) if they're not.

---

## Backlog (orthogonal items, not on the trunk)

Useful but not on the test262-100% critical path. Pick them up between
phases only when blocked, never as a primary track.

- **f64.toString(radix) trailing-digit round-half-to-even** — current
  helper truncates at 52 digits; bun rounds the 53rd. Affects
  long-fraction cases only.
- **Array<f64> literal layout** — `let xs: number[] = [1.5, 2.5]`
  currently stores f64 bits in i64 slots; need real f64 array layout.
- **SameValueZero NaN-in-Array<f64>.includes** — `includes` on
  Array<f64> for NaN should return true; FCmp(Oeq) returns false.
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
- **JSON.stringify with indent (2nd arg)** — currently the indent arg
  is ignored.
- **`typeof x === "type"` narrow** — narrows aren't yet aware of
  `typeof` shape; covered partially by P3 / P0.
- **T-35 test262 runner cargo metadata target_dir** — already
  symlink-fallback'd, nice-to-have.
- **T-32 ArrayBuffer / TypedArrays** — multi-week substrate, schedule
  when a phase needs it.
- **T-36 Date.prototype.setX statics**.
- **T-40 `new Function(body)` global ctor** — eval-子集；跟 AOT 哲学
  冲突，needs design.
- **T-41 `eval` global** — extremely deep, requires `tr` to embed
  itself; design-pending.
- **Sparse Array support** — `a[2^32-1] = X` semantic.

---

## Detoured (kept for audit trail, not active)

Probed and deferred — substrate not in place. Will resume when the
named pre-requisites land.

### P-PARSE — ES syntax parser completeness (2026-05-14)

Inserted to clear parser surface before P0 typecheck work. **Status:
items P-PARSE.1–5 absorbed into P0 / P3 / P4 work as substrate-correct
fixes; the standalone phase is no longer tracked.**

### P-COERCE-B — ToPrimitive in `+` for Struct / Function (2026-05-14)

Deferred — requires object-literal method dispatch (now landed via
P3.struct-method-dispatch) + class-instance method discovery (P4
substrate). **Resume when P4 closes** — pick up as a P5+ wedge.

### P-CLOSURE-C — closure monomorphization at call sites (2026-05-14)

Deferred — overlapped significantly with P0's tagged-Any work; P0's
tagged-Any path subsumes it. **Closed without standalone resumption**;
revisit only if a specific case shows P0 doesn't cover it.

---

## Principles (kept)

- Foundation: `docs/design-principles.md` — five-pillar rubric (高性能
  / 自研 / 正统 / 规范 / 上限优先).
- Refcount + universal heap header: `docs/refcount-architecture.md`.
- Coding rules: `.claude/rules/common/`, `.claude/rules/{rust,
  typescript}/`.
- Project-specific principles: `.claude/rules/torajs-design-principles.md`.

---

## BENCH — cross-runtime perf benchmark (cross-cutting track)

Runs on every commit alongside conformance. Same set of cases as
v3 (popcount, fib40, generic-pair-1m, array-sum-1m, closure-pipeline-1m,
promise-then-100k, ackermann, …). **Acceptance: typed-tier benches stay
green end-to-end through P0–P13.**

Cross-runtime SOTA push happens every N phase as a perf-focused
sprint, not at v1.0 gate. Detailed bench harness layout, oracle setup,
and per-case budget table live in `docs/bench.md` (TODO: extract from
v3 roadmap appendix).

---

## Historical roadmaps

`docs/roadmap-historical.md` preserves the v1 (P0–P13 foundation), v2
(33-item perf-gated), v3 (V3-XX wedge-cycle to 522/521 curated), and
v4 (test262-100% trunk) plans verbatim. Read them for the *why* of
tora's foundation. Do not read them for *what to do next* — that
lives only in this file.
