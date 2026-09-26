# torajs stdlib

torajs standard library — what's implemented today.

## Position

The stdlib's job is to give the language enough to write real programs without users reaching for the SSA layer. It complements the language's hard requirements (no GC, beat Bun on perf) by making common operations fast paths through type-specialized runtime intrinsics rather than generic JS-engine machinery.

torajs is **not** a TS6-compatible runtime. The stdlib picks the JS surface where it's clean (Math, String, Array, Map) and rejects what's incompatible with our memory model (Symbol, Proxy, eval, async with floating Promises). Naming follows TS where it works, Rust where TS would make us slow or unsound.

## Currently shipped

| namespace | members | notes |
|---|---|---|
| `console`  | `log` | `log` accepts `Type::Any` (number / string), dispatches to `print_i64` / `print_f64` / `__torajs_str_print` by SSA operand type. Borrow-style: doesn't consume its arg. |
| `Math`     | `sqrt` `abs` `floor` `ceil` `log` `exp` | Unary `f64 → f64`. Auto-promote `i64` args via SiToFp at the call site. |
| `Math`     | `pow` `min` `max` | Binary `f64 × f64 → f64`. Same promotion rule. |
| `Math`     | `PI` `E`         | Compile-time `ConstF64` constants — no runtime call. |
| `String`   | `s.length`       | Reads the `u64` length-prefix from the heap StrRepr. Returns `i64`. |

`Number`, `Boolean`, `Array`, `Map`, `Set`, `Date`, `JSON`, `Promise`, `fetch` — not yet shipped.

## Naming conventions

- Lowercase camelCase methods (`sqrt`, `parseInt`, `toString`) — matches TS.
- Constants in UPPER (`PI`, `E`, `MAX_SAFE_INTEGER`).
- Mathematical funcs accept `f64` or auto-promote integers (`Math.sqrt(2)` should just work). String funcs accept `string`.
- Conversions are explicit: no `String(123)`. Use `.toString()` once we have it.

For now, stdlib methods that can't fail panic on bad input (e.g. `Math.sqrt(-1)` returns NaN per IEEE, doesn't panic). Methods that could fail (`parseInt("abc")`) aren't shipped yet.

Not in scope:

- `Symbol` (no GC, no proxy)
- `Proxy`, `Reflect`, `Object.defineProperty` (we're statically typed)
- `eval`, `Function` constructor (no parser at runtime)
- `Promise.all` and friends — defer until async lands
- `WeakMap` / `WeakRef` (refcount, not tracing GC; weak refs are awkward)
