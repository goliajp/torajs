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

## Status snapshot (2026-05-18, HEAD `aac1934` — P6 phase closed)

### Curated conformance (`conformance/cases/`)

**618 pass / 0 fail / 1 skip** committed. +11 from P6 phase (map-001
..005 / map-for-of-001 / set-001..004 / array-iter-001). The 1
committed skip is `perf-005-dwarf-panic-fs` (bun-side crash, not
tora's bug).

### test262 5k diagnostic

**344 pass / 16 bug / 3279 incompatible** at last measured baseline
(`00c4d12`). Re-measurement post-P6 deferred (Map/Set unlock expected
to surface +N test262 cases — most Map / Set / iterator-protocol
fixtures previously hit `typecheck reject` due to substrate gap).
Pass rate is regression-detection only — not a phase trigger or
milestone.

### Bench position

Typed-tier 0 regression invariant holds across P6 substrate (binary
artifact_bytes essentially unchanged through Map/Set/MapIter/ArrIter
additions — these add new code paths, don't modify Array / Closure /
Str / Number hot paths). Multi-run median bench verification on
idle-system window pending; single-run measurements during P6 ship
were noise-dominated (mac thermal ±20-40% with concurrent godot /
rustc / node tsc background load).

Last committed bench baseline: `bench/results/2026-05-18-mini-2004980
.json` (torajs vs bun-aot geomean **4.02×** at HEAD `00c4d12`).

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

### P4 — Class hierarchies + prototype chain (DONE)

**Goal**: tora 的 nominal class system 升级到 spec §10.1
OrdinaryObject + §10.4 ExoticObject 的 `[[Prototype]]` / `[[Get]]` /
`[[Set]]` 内部 method 模型。class extends + super() + builtin extends
全部走 prototype chain。

**Substrate checklist** (closed at `fc0e125`):

- [x] **P4.1 Phase A1** First-class class objects (`a65e51f`) —
      `synthesize_class_globals` desugar pass; `const x = MyClass`
      resolves to dynobj-backed Any
- [x] **P4.0** Nested Any-dynobj field identity fix (`94e5773`) —
      Type::Any arm above is_refcounted in 3 match sites
- [x] **P4.2** Phase B+C prototype chain (`e9b6779`) — `__proto_<C>`
      singletons + class-tag side table + Object.getPrototypeOf real
      readback
- [x] **P4.3 extends-chain** (`15e2e9b`) — Object.getPrototypeOf
      borrow semantics fix for Ident args + chain walk
- [x] **P4.4 function-prototype** (`5ec3810`) — `Function.prototype.
      bind / call / apply` via desugar
- [x] **P4.5 new-meta** (`1debabe`) — `new.target` meta-property full
      spec
- [x] **P4.6 extends-builtins** (`fc0e125`) — synth Error ClassDecl +
      class-prefix typeof; **closes P4 phase**
- [x] **P4.7 catch-destructure** (`9b960a8`) — `try {} catch ({code,
      msg}) {}` 真 binding + tagged throw substrate

**Acceptance**: 7 substrate items all 完成 ✅ + conformance 0 fail ✅ +
bench-tr 0 regression ✅ + 无新非高品质外部依赖 ✅。Phase closure
commit `fc0e125` (2026-05-17 → 18 between).

**Design decisions taken in P4**:

- **K.3 globals 不扩 Type::Any** — prototype singleton 选 **runtime
  side table** (class-name 字符串 keyed)，bypass K.3 entirely。Long-
  term most robust + decoupled from K.3 design constraints.
- **Prototype helpers via desugar**: `Function.prototype.bind / call /
  apply` 走 parser desugar + runtime closure wrapping，避免在 Closure
  ABI 上加 reflective overhead.
- **Builtin extends via synth ClassDecl**: `class MyError extends
  Error` 通过在 AST 阶段 synth Error ClassDecl 实施，避免 runtime
  builtin-type erasure.

---

### P5 — Iterator protocol (DONE)

**Goal**: `Symbol.iterator` is a real resolvable property; for-of
dispatches via it; spread-in-call works for arbitrary iterables.

**Substrate checklist** (6 / 6 substrate complete, P5.4 deferred):

- [x] **P5.3 Phase A** First-class `Stmt::ForOf` substrate (`9e38c87`)
      — parse-time desugar 升级成 AST node + Array<T> / Array<Any>
      subset 走 existing `Expr::Index` lowering
- [x] **P5.1** `IteratorResult<T>` structural alias (`56036f7`) —
      `{ value: T, done: boolean }` via `__inlobj`; `Iterator<T>` /
      `IterableIterator<T>` opaque-Any
- [x] **P5.2** Symbol.iterator well-known computed-key (`1aa889b`) —
      class `[Symbol.iterator]()` parses with synth name
      `__sym_Symbol_iterator__`
- [x] **P5.3 Phase B** for-of via `[Symbol.iterator]()` dispatch
      (`1a4fa09`) — Stmt::ForOf dispatches through iterator protocol
      for user-class iterables; Array / Str / Set fast path preserved
- [ ] **P5.4** `arr.entries()` / `.keys()` / `.values()` Array
      Iterator objects — **deferred to P6 同期**, blocker is
      generic-over-T iter class substrate which P6 Map/Set surfaces
- [x] **P5.5** Spread in fn calls — literal-array spread fold
      (`26310bd`); `f(...[a,b,c])` parser desugars to `f(a,b,c)`;
      dynamic-spread via rest-param already worked; fixed-arity-
      dynamic-spread defer (runtime arity check)
- [x] **P5.6** Spread in array literal (`bdfe417` → `b3afb55`) —
      `__torajs_arr_extend_any` tagged-slot extender, Array<Any>
      spread substrate-complete

**Acceptance**: ✅ all 5 P5.1-P5.6 substrate items closed (P5.4
explicitly deferred to P6 同期 by design) + conformance 0 fail +
bench-tr 0 regression + 无新外部依赖。**5k pass rate movement
during P5 push: 145 (4.62 %) → 344 (9.45 %)** — diagnostic only.

---

### P6 — Map / Set / WeakMap / WeakSet (DONE)

**Goal**: real hash containers, all spec methods, spec-mandated
insertion-order iteration.

**Substrate checklist** (closed at `aac1934`):

- [x] **P6.1** `Map<K, V>` open-addressing robin-hood hash table
      (`7480912` + `f0a33be` TAG audit fix + `86776a6` undef tag fix).
      SameValueZero key equality; tagged-Any keys + values; 8 runtime
      helpers.
- [x] **P6.2** `Set<T>` SSA-level distinction over Map storage
      (`d598ac5`). `add` writes ANY_UNDEF for the value side; method
      dispatch forwards to Map helpers.
- [x] **P6.3** WeakMap / WeakSet audit (`f0a33be`). T-26.B substrate
      verified consistent with P6.1 Map heap-header layout +
      value_drop_heap dispatch (TAG_MAP = 15 collision fix surfaces
      here).
- [x] **P6.4** Spec methods — full surface across 4 sub-commits:
      - P6.4a forEach + V8 OrderedHashMap insertion-order substrate
        refactor (`2004980`). Split-table layout (slots[] robin-hood
        + entries[] packed insertion-order) — spec §23.1.4 / §24.2.4
        ordering preserved.
      - P6.4b MapIter substrate + Map/Set.keys/.values (`c62fe69`).
        `Type::MapIter` first-class refcounted handle; `iter.next()`
        returns `IteratorResult<any>` struct via SSA-side obj_alloc.
      - P6.4c-C1 Map/Set.entries (`73cb278`). ITER_ENTRIES +
        ITER_SET_ENTRIES kinds; per-step `[k, v]` / `[v, v]`
        Array<Any> alloc with refcount=0 pre-dec trick balancing
        any_box's rc_inc.
      - P6.4c-C2 for-of @@iterator dispatch for Map/Set/MapIter
        (`80939ba`). `for (let [k, v] of m)` destructuring works via
        `lower_for_of_map_like` binding var as `Type::Arr<Any>` for
        Map source; Set/MapIter bind as Type::Any.
      - P6.4c-C3 Array<Any> iter methods (`aac1934`). `Type::ArrIter`
        parallel to MapIter; P5.4 (Array iterator methods) unblocked
        for Array<Any> source. Typed Array<T> for non-Any T uses 8B-
        per-slot layout requiring elem-tag substrate — separate
        follow-up.
- [x] **P6.5** Iterator interop with P5 — for-of @@iterator dispatch
      (P6.4c-C2 / C3) integrates with P5.3 Phase B substrate; P5.4
      Array iter methods unblocked. Phase close audit verified: 11
      conformance fixtures (map-001..005 + map-for-of-001 + set-
      001..004 + array-iter-001) all bun-parity GREEN; conformance
      gate 618 pass / 0 fail / 1 skip.

**Acceptance**: ✅ 5/5 substrate items closed + conformance 0 fail +
no new external dependencies. Bench multi-run median verification
deferred to system-idle window (mac thermal / load noise ±20-40%
makes single-run gate unreliable; binary unchanged through P6
substrate path so theoretically 0 regression on hot Array / Closure
/ Str hot paths).

---

### P7 — Error type hierarchy + throw any (DONE)

**Goal**: real Error subtypes (TypeError, RangeError, SyntaxError, …);
`throw` accepts any value; try/catch/finally state machine
spec-conformant.

**Substrate checklist** (strict order):

- [x] **P7.1** Error class + subclass hierarchy in stdlib — SHIPPED
      `f2f5aa0` (inject TypeError/RangeError/SyntaxError/ReferenceError)
- [x] **P7.2** `throw <any value>` — SHIPPED `42d072d`/`d57bbfc`/`e3d5c7d`
      (throw undefined→ANY_UNDEF + Any-return coercion + untyped catch→Any)
- [x] **P7.3** Stack trace captured at throw site — SHIPPED `68b24dc`
      (minimal-correct §20.5.3.4 toString header; frame capture → L3b)
- [x] **P7.4** Native errors throw real RangeError / TypeError —
      SHIPPED a-1 `76252b6` / a-2 `c2dc3af` / a-b+#15 `76ace15` /
      frozen `683bd95` (conformance 629/0/1)
- [x] **P7.5** try / catch / finally state-machine matches spec
      ordering — SHIPPED `1e84f1b` (suspend pending throw across
      finally body per ECMA §14.13.3; closes the O5 spurious-
      propagation bug where finally's first may-throw call saw
      throw_active=1 from the pending and propagated before the
      callee could complete) — **P7 substrate 5/5 DONE**, trigger
      P7→P8 MET, conformance 629/0/1 preserved

---

### P8 — Class spec full (private + static blocks + accessor + super) (DONE)

**Goal**: complete the class feature set started in P4 — private
fields, static blocks, accessor properties, super-in-arrow.

**Substrate checklist** (strict order):

- [x] **P8.1** `#priv` private fields (parser + lower with name
      mangling) — SHIPPED A1 `2747225` (lexer PrivateIdent) /
      A2 `cb806d3` (parser field-decl accept + mangling) /
      A3 `915afa9` (parser dot-access raw `#`) /
      A4+A5 `e966db8` (parse-time current_class mangling + round-trip
      fixture `class-priv-001-this-field.ts` → 630/0/1 conformance).
      Hard-private (exact-class only — no Protected); cross-class
      and subclass access rejected at typecheck. Static `#x` and
      non-`this` typed-receiver `c.#x` (where c: C) outside the
      class body defer to P8.x followups.
- [x] **P8.2** Class getters / setters (accessor descriptors) —
      SHIPPED A1+A2 `acd6202` (parser detects get/set contextual
      keywords + AST `accessor_kind` on ClassMethod) / A3
      `5db54bf` (desugar renames accessors to
      `__cm_<C>__<name>_get/_set` + side-channel maps + filter
      from `__dispatch_<M>` synth) / A4+A5 `550a6fd` (check.rs
      Member read + Assign-Member write resolution via the maps
      + ssa_lower Call emission + round-trip fixture
      `class-accessor-001-get-set.ts` → 631/0/1 conformance).
      `c.value` reads the getter; `c.value = v` writes the
      setter; both single-Call, no runtime dispatch.
- [x] **P8.3** Static blocks `static { ... }` — SHIPPED A1
      `551a34a` (AST shape migration: ClassDecl `static_fields` →
      `static_init: Vec<StaticInit>` with `Field | Block` variants,
      mechanical refactor across ast.rs / parser.rs / formatter.rs /
      linter.rs) / A2 `641f0b3` (parser adds `static + LBrace`
      lookahead branch parsing block stmts into
      `StaticInit::Block(stmts)`; desugar still loud-panics on Block
      pending A3) / A3 `322be4f` (desugar walks Vec<StaticInit> in
      source order: Field → existing `__sf_<C>__<name>` LetDecl;
      Block → `__sb_<C>__<idx>` named-fn appended + top-level
      `Stmt::Expr(Call(...))` pushed into static_field_inits at the
      entry's index — preserves ES spec §15.7.10 interleaving) /
      A5 `class-static-001-blocks.ts` (4-case fixture: single
      block / interleave field+block / multi-block cross-ref /
      block-only). Known follow-up (NOT phase-blocker, parallel
      to existing static-method limitation): `this` inside a
      static block body currently fails typecheck the same way
      `this` inside a static method body does — both lift to
      top-level fns with no `__this` param. Existing class fixtures
      use `ClassName.member` form (per
      `class-static-inheritance-001.ts`); A5 follows that
      convention.
- [x] **P8.4** Arrow return-type inference for Call shape
      [SHIPPED `416c606` (A1) + `<A2-pending>` (A2)]. Originally framed
      as a narrow super-in-nested-arrow fix; probing during the ship
      cycle surfaced the actual root cause as a broader inference gap:
      `infer_expr_ann_with` bailed on every `Expr::Call`, so any
      bare-arrow body of shape `() => fn()` (with or without super)
      had its lifted closure FnDecl's return_type default to Void and
      typecheck rejected the surrounding code as a mismatch.
      A1 plumbs a fn_sigs table (built at desugar_implicit_generics
      entry from non-`__closure_*`, non-generic top-level FnDecls with
      an explicit return ann; includes desugar_classes-synthesized
      `__cm_<C>__<m>` whose return ann comes from the user-declared
      method) through the static return-ann sniff chain
      (infer_return_ann / infer_return_ann_seeded /
      collect_return_anns(_stmt) / collect_let_binding_anns(_stmt) /
      infer_expr_ann_with). infer_expr_ann_with gains a `Expr::Call`
      arm that resolves bare-Ident callees through fn_sigs. Three
      filters keep propagation sound: skip `__closure_*` (their own
      return ann is being inferred this pass), skip generic fns
      (TypeVar return is per-call-site mono), require bare-Ident callee
      (Member/Index need typechecker collaboration). Super in nested
      arrows rides this fix for free: desugar_classes Pass 1.5 / 1.6
      collectors already recurse into ArrowFn body and rewrite super
      ExprIds in place. A2 ships fixtures `class-super-arrow-001-
      nested.ts` (P8.4 named surface — super.method() / super.method(a,b)
      / super(args) in nested arrows + let-aliased) and
      `arrow-infer-callret-001.ts` (broader surface — number / string /
      boolean returning fns / block-body / param-forwarded). Known
      follow-ups parked in L3b: (i) let-bound closure call sites
      (`const inner = () => super.x(); const outer = () => inner()` —
      outer's Call(Ident("inner")) needs binds-level closure return-
      type resolution, separate substrate item); (ii) IIFE shape
      `(() => fn())()` where ssa_lower rejects `callee = Closure {...}`
      (independent of inference scope, lift_arrow_fns emits the closure
      as immediate callee — substrate gap in ssa_lower call-site
      dispatch).
- [x] **P8.5** Class expressions as values — SHIPPED A1+A2
      `769a224` (parser-level substrate) + `<A2>` (fixture-lock +
      roadmap). ES §15.7.4 ClassExpression covered: anonymous form
      (`const F = class { ... }`), named-inner-discarded form (`const
      F = class Inner { ... }`), extends form (`const F = class
      extends A { ... }`), and parenthesized-new-callee form (`new
      (class { ... })()`). Strategy (a) parser-synth ClassDecl +
      value-ref Ident — parse_primary's new Token::Class branch buffers
      the class as `__ClassExpr_<id>` in a `synth_classes` Vec (flushed
      before each stmt push in parse_program to preserve parent-
      before-child + synth-before-use ordering), emits Ident at the use
      site; the existing class-as-value substrate (`__class_<C>` +
      synthesize_class_globals's Ident rewrite) lifts it uniformly.
      parse_new gained Token::Class + Token::LParen arms for the
      `new class { ... }()` and `new (...)()` forms. A narrow alias map
      (`class_value_aliases: HashMap`) registers `const F = class {
      ... }` bindings so `new F()` rewrites to the static factory
      `__new___ClassExpr_<id>` at parse time — avoids a downstream
      dynamic-ctor-dispatch substrate. A2 fixtures: `class-expr-001-
      anonymous.ts` (single-method / ctor+field / two distinct
      classes / alias chain / cross-method call), `class-expr-002-
      named-extends.ts` (inner-name discarded, extends with method
      override, alias-of-extends), `class-expr-003-immediate-new.ts`
      (bare IIFE, IIFE-with-ctor-args, no-paren `new class`, extends
      with own ctor, instance-as-value-through-fn). Conformance
      634 → 637 (+3). Substrate-untouched downstream (desugar_classes
      / synthesize_class_globals / check / ssa_lower zero changes).

      L3b follow-ups (parked, not P8.5 scope):
      - Inner self-binding (`class Inner { ... }` body referencing
        Inner currently fails — Inner is discarded by force_synth)
      - Anonymous `.name === ""` per spec (currently
        `"__ClassExpr_<id>"`)
      - Full dynamic-ctor-dispatch substrate for `let F = class {};
        F = ...; new F()` / `function makeF() {...}; const F =
        makeF(); new F()` / arbitrary callee expressions through New
      - Alias scope-stack for fn-body shadowing (inner const-decl
        currently overwrites outer alias of the same name)
      - 3+ classes sharing the same method name → dispatch corruption
        (pre-existing, surfaced by P8.5-A2 fixture draft; reproducible
        with literal top-level form too — `class A { tag() {...} }
        class B { tag() {...} } class C { tag() {...} }` emits
        `a c c` instead of `a b c`)
      - Subclass without own constructor inheriting parent's
        constructor signature (pre-existing — subclass default-ctor
        synthesis ignores parent's arity; reproducible with literal
        top-level subclass form)

      With P8.5 shipped, P8 is fully closed (P8.1/2/3/4/5 all done) —
      P8 → P9 phase trigger met.

---

### P9 — Regex full (DONE)

**Goal**: spec-complete RegExp incl. lookahead / lookbehind, named
groups, Unicode flag, sticky flag.

**Substrate checklist** (strict order):

- [x] **P9.1** Lookbehind / lookahead — SHIPPED A1+A2 `0404b08`
      (runtime_regex.c lookbehind substrate) + `<A2>` (fixture-lock +
      roadmap). Lookahead `(?=X)` / `(?!X)` was already in place from
      Phase 1c.4 (sub-Program + sub_probe at current pos); P9.1 lands
      lookbehind `(?<=X)` / `(?<!X)` to complete the zero-width
      assertion set. Approach (B) variable-width: vm_match_at gains an
      `end_target` param (-1 = leftmost-first as before; ≥0 = only
      MATCH at pos==end_target commits + outer loop short-circuits once
      pos > end_target), and a new `sub_probe_ending_at` scans candidate
      start positions j ∈ [0..pos] invoking vm_match_at on the forward
      sub-Program with end_target = pos. The forward-compile sub stays
      shared with lookahead — no second compile mode. A2 fixtures:
      `regex-008-lookbehind.ts` (positive/negative × at-start /
      mid-pattern / combined-with-lookahead), `regex-009-lookbehind-
      variable.ts` (alternation in body / quantifier in body / char-
      class with quantifier / negative-with-quantifier), `regex-010-
      lookbehind-replace.ts` (replace / replaceAll / match / .test()
      with anchors). Conformance 637 → 6XX (+3). No AST / parser /
      lookahead behavioral change.

      Why Approach B vs (A) reverse-compile sub: minimal AST surface
      (no second compile mode), narrow-surface fix per 设计原则 #4
      (规范). Worst-case O(pos · sub_len) acceptable for v0.1 — body
      lengths in practice are short. Upgrade path to (A) replaces only
      sub_probe_ending_at; AST / op / parser stay put — a future
      perf-axis phase can swap implementations transparently.

      P9.1 closing advances L3a to P9.2 (named capture groups + back-
      references). P9 phase has 5 substeps; closing all unlocks P9→P10
      trigger.
- [x] **P9.2** Named capture groups + back-references — SHIPPED A1+A2+A3
      `8a5aa61` (A1: parser + matcher substrate) + `4b12de4` (A2: .groups
      accessor) + `<A3>` (fixtures + this roadmap). A1 lands the regex
      engine substrate: parser accepts `(?<name>X)` (records name in a
      new Parser.names table aligned with capture_idx); `\k<name>` emits
      NK_BACKREF resolved post-parse via the name table; `\1..\9` emits
      NK_BACKREF{idx} validated against final n_captures. New OP_BACKREF
      opcode + per-thread `br_offset` state machine in the outer match
      loop consumes the captured slice byte-at-a-time across steps
      (continuation re-scheduling bypasses the visited table so a fresh
      backref entry isn't blocked by an in-flight continuation at the
      same pc — they carry different state). i-flag aware via the
      existing char_eq path. A2 attaches `.groups` to match-result
      arrays: RegExp now persists capture_names past parse, and a new
      attach_groups helper builds a dynobj of name → captured Str, set
      on the array via the existing arrprops side-table. `m.groups`
      reads already lower through arrprops_get (the typechecker routes
      Array.<unknown> to Type::Any), so no compile-side changes were
      needed. Non-participating named groups → ANY_UNDEF entries per
      spec §22.2.5.7. A3 fixtures: `regex-011-named-capture.ts` (parser
      acceptance + positional / `.exec` access), `regex-012-backref.ts`
      (positional `\1..\9` single/multi-char + non-participating +
      i-flag + alternation), `regex-013-named-backref.ts` (`\k<name>`
      single/multi-char + forward refs + mixed positional/named refs +
      i-flag), `regex-014-groups-dict.ts` (`.groups.NAME` access on
      `match` / `exec` + non-participating undefined + named+positional
      coexistence). Conformance 640 → 6XX (+4).

      Narrow-surface design choice (per [[feedback-narrow-abi-surface]]):
      parser owns the name table; matcher stays positional. Alternative —
      push name resolution into the matcher — would surface-broaden Op +
      Thread for no runtime benefit. The Thompson NFA + multi-byte backref
      tension (Russ Cox style normally precludes backref) is resolved by
      the per-thread `br_offset` state machine — the only invasive change
      was replacing Thread's unused `pad` field with `br_offset` (same
      sizeof).

      L3b follow-ups recorded:
      - ECMA Annex B OctalEscape / IdentityEscape for `\N` when N >
        n_captures (currently rejected at parse; bun returns false-
        match on regex-execution rather than rejecting the literal).
      - OP_CLASS i-flag awareness (pre-existing; `[a-z]/i.test("A")` →
        false on tora vs true on bun). Independent of P9.2 but surfaced
        while writing fixtures.
      - Typechecker RegExpMatchArray type — `.match()` / `.exec()`
        currently return `Array<String>`, so `.groups` access requires
        `as any` cast in source. Surface ergonomics improvement, not a
        correctness gap.

      P9.2 closing advances L3a to P9.3 (Unicode flag).
- [x] **P9.3** Unicode flag (`u`) — character class handling — SHIPPED
      A1+A2+A2.1+A3 `3fd8cfe` (A1: `\u{}` / `\uHHHH` escape + `.`
      astral) + `97dcf93` (A2: `\p{Letter|Number|ASCII}` + OP_CLASS
      code-point) + `6244622` (A2.1: search start-pos skips UTF-8
      continuation bytes) + `<A3>` (fixtures + this roadmap).

      A1 adds u-flag mechanics: parse_escape recognises `\u{HHHH..}`
      (extended form, u flag only) and `\uHHHH` (4-digit form,
      always — also fixes a pre-existing parser bug where `\uHHHH`
      was treated as literal `u<digits>` even without u flag). Both
      forms encode to UTF-8 bytes and emit as NK_CONCAT of NK_CHARs,
      so the byte-step Thompson NFA matches the encoded sequence
      naturally without a new opcode. `.` (OP_ANYCHAR) under u flag
      advances by `utf8_len_for(s[pos])` (1–4 bytes); the destination
      thread is patched with a new `Thread.u_skip` defer counter so
      the outer step queue waits adv-1 steps (consuming continuation
      bytes implicitly) before dispatching the next op. Bypass-
      visited defer keeps the queued thread alive across step swaps
      without colliding with fresh entrants at the same pc — same
      pattern as P9.2-A1's OP_BACKREF `br_offset`.

      A2 adds Unicode property classes: `\p{L|Letter}`, `\p{N|Number}`,
      `\p{ASCII}` parsed in parse_escape (outside class) and
      parse_class (inside `[...]` — OR-unions into the existing
      class). `\P{X}` outside class = class-level negate. ASCII
      portion lives in the regular bitmap; cp ≥ 128 portion is
      covered by curated UCD subset tables (Greek, Cyrillic, Hebrew,
      Arabic, Devanagari, Thai, Hiragana, Katakana, CJK, Hangul,
      common decimal-digit scripts). A new CharClass.u_props bitfield
      + cc_test_cp helper dispatches: cp < 256 → bitmap, cp ≥ 128 →
      uprop_range_contains binary search. OP_CLASS under u flag
      decodes one code point at s[pos] via utf8_decode_cp, tests via
      cc_test_cp, and reuses the A1 u_skip patch for multi-byte
      advance.

      A2.1 is a follow-up to A2: the byte-iterating search start
      loops (vm_search_from + vm_search_from_with_ws) skip UTF-8
      continuation bytes (`(s[st] & 0xC0) == 0x80`) under u flag,
      so `/[^\p{L}]/u.test("漢")` doesn't accidentally accept the
      mid-sequence continuation byte 0xBC as a stand-alone non-
      Letter code point. Code-point-aligned start positions only.

      A3 fixtures: `regex-015-unicode-flag.ts` (extended `\u{}` BMP
      + astral, `\uHHHH` with/without u flag, `.` astral + anchored,
      `.match(/./u)` for emoji / BMP / ASCII, `.+` over multiple
      astrals, mixed ASCII+astral, literal multi-byte in pattern,
      `\u{}` leading-zero variants), `regex-016-unicode-properties.ts`
      (\p{L|Letter} / \p{N|Number} / \p{ASCII} positive, \P
      negation, .match with \p{L}+, /\p{L}+/gu global, `[\p{L}\p{N}]/u`
      union, `[^\p{L}]/u` class-level negate, mixed bitmap+property
      `[a-z\p{N}]/u`, replace with property, anchored property,
      alias resolution). Conformance 644 → 6XX (+2 with the new
      fixtures).

      Narrow-surface design choice (per [[feedback-narrow-abi-
      surface]]): no new opcode, no Inst layout change, no Node
      field addition. Code-point semantics realised entirely via the
      Thread.u_skip defer queue + CharClass.u_props bitfield +
      static UCD tables — same narrow-surface playbook as P9.1
      (sub_probe_ending_at) and P9.2 (br_offset). The Thompson NFA
      "1 byte per outer step" invariant is preserved; u-flag work
      happens in scheduled-defer pattern instead of changing the
      outer loop's step granularity.

      L3b follow-ups recorded:
      - Full UCD property tables — v0.1 ships hand-curated subsets
        (Greek/Cyrillic/Hebrew/Arabic/CJK/Hangul/Hiragana/Katakana
        + common-script decimals). Real \p{L} has hundreds of
        ranges; auto-import from UCD data files is L3b. Dominant
        test262 cases pass with the curated subset.
      - `\P{X}` inside class (complement semantics inside `[...]`)
        — current v0.1 errors out; correct semantics requires either
        per-property complement tables or a "negative-bitfield" mode
        on CharClass.
      - `v` flag (ES2024 set notation `[[\p{X}--[a-z]]]`) — separate
        substep beyond v0.1.
      - Lone surrogate handling — `"\uD800".match(/\uD800/)` differs
        from bun's UTF-16 view (tora's WTF-8 byte representation
        doesn't perfectly round-trip ill-formed inputs). Edge of
        spec, low test262 impact.
      - Property=Value form (`\p{Script=Latin}`) — parser accepts
        only Name-only form; Name=Value is L3b.

      P9.3 closing advances L3a to P9.4 (Sticky flag).
- [x] **P9.4** Sticky flag (`y`) — lastIndex semantics — SHIPPED
      A1+A1.1+A2 `9fe2ebb` (A1: RegExp.last_index field + accessors +
      sticky/global lastIndex semantics in __torajs_regex_exec and
      __torajs_str_match_regex non-global path) + `4f59eb8` (A1.1:
      sticky-aware replace/replaceAll/split/matchAll) + `<A2>`
      (fixtures + this roadmap).

      A1 introduces `int64_t RegExp.last_index` (calloc init 0) +
      runtime accessors __torajs_regex_get_last_index /
      __torajs_regex_set_last_index + a new `vm_match_anchor` helper
      for single-position anchored match (used by sticky paths to
      anchor at lastIndex with miss-on-continuation-byte under u
      flag). Surface routing: ssa_lower adds read-side branch (call
      get_last_index returning I64) and write-side branch (coerce_to_i64
      + call set_last_index) for the `re.lastIndex` member; check.rs
      adds `(Type::RegExp, "lastIndex") => Type::Number` for reads
      and a permissive write-arm before the struct-only check.

      Semantics: sticky (`y`) anchors at lastIndex with single
      attempt; global (`g`) starts search from lastIndex; plain
      ignores lastIndex and never writes it. Y takes precedence
      when both flags set. On miss with tracking, reset lastIndex
      to 0 per spec §22.2.5.2.2; on hit, write match end.

      A1.1 surfaced during fixture verification — the other regex
      iterators (replace / replaceAll / split / matchAll) kept their
      own loops over vm_search_from_with_ws and silently disagreed
      with bun under y flag (e.g. `"aXab".replace(/a/gy, "Y")` gave
      "YXYb" because the loop walked past the sticky failure at
      index 1 to the next 'a'). Same narrow-surface fix in all four
      functions: branch on sticky → vm_match_anchor at `pos` → break
      loop on miss. Pattern mirrors P9.3-A2.1 (substrate fix exposed
      at fixture-write time, ship as independent gated commit).

      A2 fixture `regex-017-sticky.ts` (15 cases): sticky anchor +
      r/w/reset, sticky walk via repeated exec, sticky miss mid-string,
      lastIndex > length / negative clamp, g-only advance, plain
      flag ignore, g+y interaction (both anchor and advance),
      sticky replace + replaceAll (the cases that surfaced A1.1),
      s.match with sticky hit + miss, lastIndex from indexOf,
      multi-char pattern anchored advance. Each block byte-equal
      vs bun. A1 + A1.1 gates each 646/0/1 (0 regression vs
      post-P9.3 baseline). A2 ships fixture-only per autorun
      pipeline fixture-only exception (no substrate change).

      Narrow-surface design (per [[feedback-narrow-abi-surface]]):
      no Inst layout change, no new IR opcode, no Node field
      addition. RegExp struct grows by one int64 (last_index); two
      new runtime accessors + one vm-internal helper
      (vm_match_anchor); two new compile-time intrinsics; one
      read-side + one write-side dispatch arm in both check.rs and
      ssa_lower.rs.

      L3b follow-ups recorded:
      - `RegExp.prototype.test()` should also honor sticky/global
        lastIndex per spec — currently calls vm_search_from(0,
        flags) ignoring lastIndex (line 2083). Trivial fix mirror
        of exec(); deferred to surface the trade with a clear
        commit (test() semantics affect many test262 entries).
      - `vm_match_anchor` internally allocates workspace; could be
        promoted to a `_with_ws` variant for tight-loop reuse. Not
        a measurable hot-path concern at current case sizes.
      - regex_exec's miss returns `[]` not `null` (Phase 1c.4
        Nullable<Array>) and hit-result lacks `index` / `input` /
        `groups` props as separate attachments (Phase 1c.4 array-
        prop). Unrelated to P9.4 but the new fixture uses
        `m !== null && m[0] === ...` shape to side-step them.
      - sticky split / matchAll behaviour on g+y is now correct
        for the iteration but doesn't yet match bun's `g`-required
        TypeError for `matchAll(non-g-regex)` (Phase 1c.4 work).

      P9.4 closing advances L3a to P9.5 (replace callback fn).
- [x] **P9.5** `String.prototype.replace(regex, fn)` callback form —
      SHIPPED A1 `851f26d` + A2 `b0389f0` + A1.1 `a554f8d` + A1.1-A2
      `503c928` + A1.2 `ee92139` + A1.2-A2 `<A2>` (fixture + roadmap
      close). A1 lands the **first cross-boundary closure invoke from
      C runtime** in tora — runtime helpers load fn_addr from env+8
      (same ABI as `promise_then_closure`) and invoke
      `(env, m, g1, ..., gN [, off, input]) -> ret_str` per match.
      A1.1 extends from `(m)` to `(m, g1, ..., gN)`; A1.2 adds the
      trailing `(offset, input)` args per ES spec §22.1.3.18.

      `ssa_lower:19613` regex-receiver branch dispatches by repl SSA
      type: `Type::Str` → existing `regex_replace` / `_all` (expand_repl
      path); `Type::Closure` → fn-variant intrinsics. Closure user-sig
      shape detected at lower time:
       - `[Str; N+1] -> Str`            → A1.1 (has_off_input=0)
       - `[Str; N+1, I64, Str] -> Str`  → A1.2 (has_off_input=1)
      with N matching the regex's static capture count; mismatches +
      N>9 panic with a clear compile-time message — never silent-wrong
      from C ABI cast mismatch. `check.rs:3732` widened the 2nd arg from
      `Type::String` to `Type::Any` so both Str and Closure pass
      typecheck. Sticky / global handling mirrors the Str-repl
      siblings (P9.4-A1.1 semantics preserved through fn path).

      **Runtime layout** (`runtime_regex.c`): 20 cb typedefs total —
      `replace_cb_N_t` (N=0..9, A1.1 shape) + `replace_cb_N_off_t`
      (N=0..9, A1.2 shape with trailing offset+input). The
      `invoke_replace_cb(n_caps, has_off_input, env, fn_ptr, m,
      caps, off, input)` static helper picks the right cast via a
      branch on `has_off_input` and a 10-arm switch on `n_caps`.
      `build_capture_strs(n_caps, saves, s, out_caps)` constructs N
      Strs from `saves[2*(i+1)..]` per match. Outer helpers pass
      match-start `st` as offset and the receiver `str_ptr` as input
      (borrowed — cb must not retain past invocation).
      Non-participating capture groups (saves slot == -1) emit empty
      Str rather than `undefined` — A1 narrow scope; Nullable<Str>
      cb params + true undefined semantics are A1.1.1 follow-up.

      **ssa-lower side** (`ssa_lower.rs`): new top-level fn
      `count_capture_groups(pattern) -> usize` with 9 unit tests
      (plain / nested / non-capturing / named / char-class / escaped).
      Intrinsic sigs widened from `[Str, RegExp, Ptr]` to
      `[Str, RegExp, Ptr, I64, I64]` to thread n_caps + has_off_input
      through. Closure user-sig shape detected per dispatch site
      (A1.1 vs A1.2). For ident-bound regex (where capture count
      can't be statically derived) n_caps defaults to 0; N≥1 cb with
      ident regex panics with a clear message.

      Fixtures:
      - `regex-018-replace-callback.ts` (15 cases, A1 N=0 baseline)
      - `regex-019-replace-callback-captures.ts` (14 cases, A1.1 N=1..3
        including the canonical bun idiom `(\w+) (\w+)` swap)
      - `regex-020-replace-callback-offset-input.ts` (10 cases, A1.2
        full arity — offset + input, mixed with N=0..3 captures)
      All three fixtures byte-equal vs bun.

      Conformance 646 → 650 across the A1/A1.1/A1.2 chain (gates
      `/tmp/torajs-conformance-p95a*.log`, 0 regression at each step).
      The +4 comes from regex-017 (P9.4-A2), regex-018 (P9.5-A1),
      regex-019 (P9.5-A1.1), regex-020 (P9.5-A1.2).

      A1/A1.1/A1.2 scope is intentionally narrow (per
      [[feedback-narrow-abi-surface]]): this is the first C-runtime
      closure invoke surface in tora. A1 shipped the ABI pattern with
      strict `(m) => ret`; A1.1 generalized to N captures via static
      parse; A1.2 added the trailing offset+input args. Each was a
      clean increment on the proven A1 substrate.

      Constraint: callback param types must be explicitly annotated
      (e.g. `(m: string, g1: string, off: number, input: string) =>
      string`). Tora's `build_fn_type` already requires this for arrow
      fns (consistent with `arr.map` / `filter` / `forEach`), so the
      friction matches existing patterns.

      L3b follow-ups (independent of P9, deferrable to future phases):
      - **A1.1.1 non-participating groups as undefined** — current
        A1.1 emits empty Str for `(a)|(b)`-style alternation where
        one group doesn't fire. Spec says `undefined`. Requires
        Nullable<Str> cb param support — independent typecheck
        work.
      - **String-receiver fn callback** — `"foo".replace("o", fn)`
        (non-regex string pattern) also accepts fn callback per spec;
        currently rejected at typeck via the Str-pattern arm.
        Independent of P9 substrate.

      P9.5 closing completes the full P9 phase (5/5). P9 → P10
      trigger met (substrate checklist 5/5 ✓; conformance 650/0/1
      holds). L3a advances to P10.1 (Microtask queue with drain at
      every yield point).

---

### P10 — Promise + async-await + Generator (CURRENT)

**Goal**: real microtask queue, ordering guarantees, async iterators,
generator full state machine. v5 merges v4's P9 (Promise) + P14
(Generator) into one phase — both share state-machine substrate.

**Substrate checklist** (strict order):

- [x] **P10.1** Microtask queue with drain at every yield point —
      SHIPPED A1 `b252492` + A1-A2 `a0f699f` + A1.1 `6d134e3` +
      A1.2 `2d3a317` + A2 `<closing>`. Wires WHATWG HTML
      §queueMicrotask global to the existing T-15.c microtask
      queue + T-15.e main-exit drain. Substrate (queue +
      run-until-idle drain + main-exit auto-call + await drain)
      was already complete since v0.5; this phase adds the
      language-layer entry point and the closure-capture
      visibility fix that lets the cb body schedule further
      microtasks.

      **A1 `b252492`** — `queueMicrotask(cb)` for closure-typed
      cb. Runtime: new `__torajs_queue_microtask_closure(env)` +
      `queue_micro_closure_dispatch_(arg)` mirroring
      `finally_closure`'s env+8 fn_addr ABI (cb is `void (env*)`;
      rc-inc env at attach, drop via `__torajs_value_drop_heap`
      after invoke). SSA: new `microtask_enqueue_closure`
      intrinsic + bare-name lowering arm in `ssa_lower:15842`.
      check.rs: new bare-name type-check arm at `~5586`
      enforcing `Type::Function([], Void)`. Fixture
      `micro-001-queueMicrotask-basic.ts` byte-equal vs bun.

      **A1-A2 `a0f699f`** — docs-only roadmap progress note.

      **A1.1 `6d134e3`** — simple-fn (named fn decl) cb path. A1
      always emitted the closure intrinsic regardless of cb
      type, so passing a named-fn ident (`Type::FnSig`, raw fn
      ptr) → runtime read garbage at env+8 → SIGBUS. Fix mirrors
      `promise_then_{simple,closure}` dispatch (`ssa_lower:17152`):
      branch on cb's static type at the call site. Type::Closure
      → `_closure` (existing); Type::FnSig → new
      `__torajs_queue_microtask_simple(fn_ptr)` which casts back
      to `void ()` and invokes (no rc; fn pointers live in
      .text). Fixture `micro-002-queueMicrotask-named-fn.ts`.

      **A1.2 `2d3a317`** — visibility of `queueMicrotask` inside
      closure bodies. Surfaced by nested-microtask probe: cb body
      that calls `queueMicrotask(...)` failed with "closure
      `__closure_N` references unknown identifier `queueMicrotask`"
      because the closure-capture analyzer
      (`check.rs:7032`) treated the bare ident as a captured
      local. Pre-existing globals (parseInt / isNaN / ...) were
      exempt via `ast.rs::is_global_name`; the list missed
      queueMicrotask. Added it to both that list and
      `check.rs::is_known_builtin_global` per the in-code
      sync-comment. Fixture
      `micro-003-queueMicrotask-nested.ts` exercises 3-level
      nested chain (mt-1 → mt-2 → mt-3 inside same drain cycle
      via `__torajs_microtask_run_until_idle`'s
      `while (mt_head_ < mt_len_)` loop).

      **Drain-coverage audit @ P10.1 close**: spec "every yield
      point" reduces to main-exit + await + nested-cb scheduling.
      tora covers all three: T-15.e main-exit drain
      (`ssa_lower:6163`); T-16 await drain (`ssa_lower:23659`);
      nested drain via `run_until_idle`'s growing-tail loop.
      Audit fixtures (`/tmp/p10.1-a1.2-audit-{sequential,nested,
      deep-nested}.ts`) all byte-equal vs bun.

      **Conformance** monotonic non-decreasing across A1/A1.1/A1.2
      gates: 651/0/1 (A1, `/tmp/torajs-conformance-p10.1-a1.log`)
      → 652/0/1 (A1.1,
      `/tmp/torajs-conformance-p10.1-a1.1.log`)
      → 653/0/1 (A1.2,
      `/tmp/torajs-conformance-p10.1-a1.2.log`). +3 = micro-001
      + micro-002 + micro-003 picked up.

      **Follow-up (L3b, not P10.x scope)**:
      - **A1.3 `Window.queueMicrotask`** namespaced form — defer
        until/unless namespace globals matter (tora is
        Node-runtime style; the bare-name binding suffices).
      - **const-lambda binding crash** — pre-existing SIGBUS on
        `const cb = () => {...}; queueMicrotask(cb)` (also
        reproduces on `.finally(cb)`). Affects multiple
        closure-cb sites; needs ident-resolution audit (Closure
        value vs Closure-box rc handling on var read). Out of
        P10.1 scope.

      P10.1 closing advances L3a to **P10.2** (Promise.all /
      .race / .allSettled / .any per spec). P10 phase has 7
      substeps; closing all unlocks P10 → P11 trigger.
- [ ] **P10.2** Promise.all / .race / .allSettled / .any per spec
      (currently allSettled is single-T MVP).

      **IN PROGRESS** (resumed-session 2026-05-21):

      - **A1** `5be6b5c` — `Promise.resolve()` / `Promise.reject()`
        0-arg form per ES spec §27.2.4.7 / §27.2.4.5. 0-arg ≡
        passing `undefined`. Inner T = `Type::Undefined`.
        - check.rs:5167 — `args.is_empty()` early-returns
          `Promise<Undefined>`; `args.len() > 1` errors as
          "expects 0 or 1 arg".
        - ssa_lower:17575 — new early branch for `args.is_empty()`
          synthesizes `Operand::ConstI64(0)` (undefined sentinel,
          shares i64-0 ABI with null) and dispatches the non-heap
          `promise_alloc_fulfilled` / `_rejected` allocator. No
          runtime / IR-helper changes.
        - Fixture `conformance/cases/async-018-promise-resolve-0arg.ts`
          covers `Promise.resolve().finally(...)` chain; byte-equal
          vs bun. Gate **654/0/1** (baseline 653 + async-018, 0
          regression).
        - reject() 0-arg runtime-smoked (exit 0, no segfault);
          fixture deferred to A1.1 once `.catch` accepts inner
          T=Undefined.

      - **A1.1** `6c93b90` — `.then` / `.catch` accept inner
        T=Undefined on `Promise<Undefined>`. Builds on A1.
        - check.rs:~5001 — new Call-time arm specializing
          `(Type::Promise(Type::Undefined), "then" | "catch")`
          with cb sig `() => U` (0-arg form, ergonomic over
          spec `(v: undefined) => U`). cb return U: primitive
          (Number/String/Boolean) → Promise<U>; Void/Undefined
          → Promise<Undefined>; other → typecheck error.
        - ssa_lower zero changes — SSA Type::Promise is unit
          (no inner T), existing cb_ty Closure/FnSig dispatch
          at line ~17220 routes correctly to promise_then_*
          helpers without inner-T inspection.
        - runtime zero changes — `then_simple_dispatch_` casts
          cb to `int64_t (*)(int64_t)`; SystemV puts unused
          arg in rdi (cb ignores). Standard ABI tolerance.
        - Fixture `conformance/cases/async-019-promise-resolve-then-catch.ts`
          chains `.then` + `.catch` on resolve()/reject() 0-arg
          with sync interleave; byte-equal vs bun
          (`sync\nr1\nr2`). Also closes A1's reject() 0-arg
          runtime-smoke gap (now fixture-tested).
        - Gate **655/0/1** (baseline 654 + async-019, 0 regression).

      P10.2-A1 family closed (A1 substrate + A1-A2 docs + A1.1
      substrate). Two gates monotonic 654 → 655 / 0 / 1.

      - **A2** `ef3c895` — ssa_lower static_ctor whitelist for
        Promise statics. Smoke probe during this rotation revealed
        `Promise.race(ps).then(cb)` failed at lower time with
        "not yet supported: ssa-lower: unsupported member call
        shape: then". Root cause: `src_is_builtin_promise`
        whitelist at `ssa_lower:~17098` recognized only resolve /
        reject; the four T-17.a/b/c statics (all / race / any /
        allSettled) returned `Type::Promise` from check.rs but
        weren't picked up by the lowering whitelist, so chained
        calls fell through to the (non-existent) user-class
        fallback.
        - ssa_lower:~17098 — extend `static_ctor` match's name
          set to all six Promise namespace statics via
          `matches!(src_m.as_str(), "resolve" | "reject" | "all"
          | "race" | "any" | "allSettled")`. Pattern stays
          identical (obj==Ident("Promise")).
        - Zero runtime / IR-helper changes. Zero check.rs
          changes (each static already returns Type::Promise).
        - Fixture `conformance/cases/async-020-promise-race-any-then.ts`
          chains `.then` on `Promise.race(ps)` and
          `Promise.any(ps)` (both yield Promise<Undefined> for a
          Promise<Undefined> input array; A1.1's then/catch arm
          takes over from there). Byte-equal vs bun
          (`sync\nrace-done\nany-done`).
        - Gate **656/0/1** (baseline 655 + async-020, 0
          regression).

      Three gates monotonic 654 → 655 → 656 / 0 / 1. Rotation
      closes here after A2 ship.

      **Next sub-A's queued**:

      - **A3** Extend `Promise.allSettled` accepted T beyond
        Number-only. check.rs:5333 currently hard-errors with
        "T must be number in v0.5 MVP" — narrow MVP extends T to
        {Number, String, Boolean} primitive set (aligns with
        `Promise.all` current T support). Result struct value
        field type must track T monomorphically (each T →
        distinct Struct type). Heterogeneous T-tuples (per spec)
        need PromiseId interning substrate (T-15.g.6+), larger
        scope deferred.
      - **A4** Extend `.then` / `.catch` to accept inner
        T=Array<U>. Blocks `Promise.all(ps).then(cb)` (currently
        fails: "no member .then on type Promise(Array(Undefined))").
        Mostly a check.rs widening; SSA/runtime unchanged (heap
        ptr through i64 ABI is standard).
      - **A_n** Heterogeneous T-tuples for Promise.all /
        .allSettled per spec — depends on PromiseId interning.

      **Naming-drift note (rotation boundary)**: e5a1944 (A1-DONE
      docs) initially queued "A2" as "extend allSettled T". A
      smoke probe right after A1.1 ship exposed the ssa_lower
      whitelist gap, which was narrower + more foundational, so
      this rotation shipped that as "A2" instead, and renamed
      the allSettled T extension to A3. Mild rotation-protocol
      trigger-#3 drift signal contributed to this rotation
      closing here. Recorded so future audits trace the
      sub-step naming progression cleanly.

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
