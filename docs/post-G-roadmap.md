# Post-G Roadmap

`docs/100-percent-plan.md` covers Phase B–G (variadic, big types, iterators,
pattern, OO basics, misc). This file picks up where it ends — at 262/262
ports — and lays out the architectural pieces in **bottom-up order**: each
phase rests on the one before, no out-of-order shipping.

---

## Phase H — Polymorphism foundation (vtable / runtime dispatch)

The current OO is **compile-time**: `instanceof` walks the static struct
type's parent chain; `obj.method()` resolves via `method_to_class` at
desugar time; method override is rejected. Heterogeneous arrays of
`Animal[]` holding mixed `Dog`/`Cat` instances do not work — every read
sees the static type only.

H promotes the object header to carry a runtime **type id**, then a
**vtable pointer**, then virtual-dispatches every method call.

| step | scope | commits |
|---|---|---|
| H.1 | Object header reserves `type_id: u32` at offset 0; field offsets shift by 8. `__new_C` writes the per-class id. | 1 |
| H.2 | `instanceof` reads runtime `type_id` and walks the static parent map. Heterogeneous arrays correct. | 1 |
| H.3 | Per-class vtable: const fn-ptr table, method names interned to slot indices. Vtable ptr stored in object header (offset 8 once H.1 lands). | 1 |
| H.4 | `obj.method()` desugar emits `vtable[slot](obj, ...)` indirect call. Static dispatch fallback when receiver type is final / sealed. | 1 |
| H.5 | Subclass vtable inherits parent's slots, overrides own. Lift the desugar-time override rejection. | 1 |
| H.6 | `super.method()` bypasses vtable — direct `__cm_Parent__method` call. | 1 |
| H.7 | Polymorphic `Animal[]` test with mixed `Dog`/`Cat` + override + super. | 1 |

Total: ~7 commits. Layout shift in H.1 is the big atomic change; rest is
incremental.

---

## Phase I — Iterator protocol

Once vtable exists, "iterable" becomes "vtable has a `next` slot."

| step | scope | commits |
|---|---|---|
| I.1 | User `class C { next(): { value: T, done: boolean } }` works in `for (let x of c)`. | 1 |
| I.2 | `Array<T>` exposes a method-style iterator that follows the same protocol. | 1 |
| I.3 | `for-of` loop sugar over user iterables (already works on Array; extend to user types). | bundled with I.1 |

Total: ~2 commits.

---

## Phase J — Generators (`function*` / `yield`)

State-machine lowering. Each `yield` becomes a state. Generator function
returns an iterator object whose `next()` resumes the machine. Builds on
Phase I (iterator protocol) for the iterable shape.

| step | scope | commits |
|---|---|---|
| J.1 | Parser: `function*` syntax + `yield` expression. | 1 |
| J.2 | SSA lower: rewrite generator body as a state machine. Locals lift to a state struct. | 2-3 |
| J.3 | Generator returns user-visible iterator object — integrates with for-of. | 1 |
| J.4 | `yield*` delegation. | 1 |

Total: ~5 commits. The state-machine rewrite is the meatiest piece in the
roadmap.

---

## Phase K — Modules (`import` / `export`)

Independent of H–J. Multi-file linking restructures the build pipeline but
doesn't touch the SSA layer's per-function shape.

| step | scope | commits |
|---|---|---|
| K.1 | Lexer + parser handle `import` / `export` declarations. | 1 |
| K.2 | Per-file AST collection; cross-file symbol table. | 1 |
| K.3 | Multi-file lower into a single LLVM module; cc-link unchanged. | 1 |
| K.4 | Type alias / class / fn visibility honored across files. | 1 |

Total: ~4 commits.

---

## Phase L — Promise + async / await

Builds on Phase J's state machine. Promise itself is a simple class with
a callback queue; `async fn` is a generator-shaped state machine that
hands its eventual value to a Promise.

| step | scope | commits |
|---|---|---|
| L.1 | `Promise` class: resolve / reject / then / catch / finally. No event loop yet — callbacks fire eagerly when state transitions. | 2 |
| L.2 | `async` function syntax + body lowering — reuse generator state machine. Each `await` is a resume point. | 2-3 |
| L.3 | Microtask queue (poll-mode, drained at end of each top-level statement). | 1 |
| L.4 | Top-level await (entry-point only, not arbitrary blocks). | 1 |

Total: ~6 commits.

---

## Phase M — Regex

Independent. Can land any time after H.

| step | scope | commits |
|---|---|---|
| M.1 | Lexer: `/pattern/flags` literal recognition (with regex-vs-divide disambiguation). | 1 |
| M.2 | Thompson-construction NFA compiler. | 2 |
| M.3 | `String.match` / `search` / `replace` / `split` accept regex args. | 1 |

Total: ~4 commits.

---

## Phase N — Symbol primitive

| step | scope | commits |
|---|---|---|
| N.1 | `Symbol` primitive type — interned unique strings, distinguishable at runtime. | 1 |
| N.2 | `Symbol.iterator` well-known symbol — dispatch hook in for-of. | 1 |

Total: ~2 commits.

---

## Phase O — BigInt

| step | scope | commits |
|---|---|---|
| O.1 | `bigint` primitive (i128 internal); `123n` literal suffix. | 1 |
| O.2 | BigInt arithmetic + comparison + mixed-with-Number coercion rules. | 2 |

Total: ~3 commits.

---

## Phase P — Stragglers

Phase G leftovers + post-roadmap polish. Pick by impact, not order.

- `Object.assign` / `entries` / `fromEntries`
- `Array.toSpliced`
- `String.normalize` (clone helper)
- Misc test262 ports as they surface

---

## Why this order

`H.1` (object header) is the **lowest-level change** — every later phase
that touches a heap object inherits the new layout. Doing it first means
no later phase has to retrofit the shift.

`I` (iterator) needs `H` (vtable) so user types can plug in dispatch.

`J` (generator) needs `I` (iterator) for the return shape.

`L` (async) needs `J` (state machine) for the resume mechanism.

`K` / `M` / `N` / `O` are structurally independent and can interleave.

`P` is a sweep — wait until the platform stops shifting.
