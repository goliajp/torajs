# torajs stdlib

Standard library design notes — the surface, the dispatch model, and how to add a new piece.

## Position

The stdlib's job is to give the language enough to write real programs without users reaching for the SSA layer. It complements the language's hard requirements (no GC, beat Bun on perf) by making common operations fast paths through type-specialized runtime intrinsics rather than generic JS-engine machinery.

torajs is **not** a TS6-compatible runtime. The stdlib picks the JS surface where it's clean (Math, String, Array, Map) and rejects what's incompatible with our memory model (Symbol, Proxy, eval, async with floating Promises). Naming follows TS where it works, Rust where TS would make us slow or unsound.

## Currently shipped (post-stdlib slice 1)

| namespace | members | notes |
|---|---|---|
| `console`  | `log` | `log` accepts `Type::Any` (number / string), dispatches to `print_i64` / `print_f64` / `__torajs_str_print` by SSA operand type. Borrow-style: doesn't consume its arg. |
| `Math`     | `sqrt` `abs` `floor` `ceil` `log` `exp` | Unary `f64 → f64`. Auto-promote `i64` args via SiToFp at the call site. |
| `Math`     | `pow` `min` `max` | Binary `f64 × f64 → f64`. Same promotion rule. |
| `Math`     | `PI` `E`         | Compile-time `ConstF64` constants — no runtime call. |
| `String`   | `s.length`       | Reads the `u64` length-prefix from the heap StrRepr. Returns `i64`. |

`Number`, `Boolean`, `Array`, `Map`, `Set`, `Date`, `JSON`, `Promise`, `fetch` — not yet shipped.

## Architecture

### Built-in global objects (current model)

Hardcoded in `check.rs` as `Type::Object("<name>")` returned by Ident lookup, with member resolution baked into the typechecker's `Expr::Member` arm. The lowerer uses parallel logic in `resolve_callee` (for method calls) and `lower_member` (for constants + field access) to map `Math.<method>` → an interned intrinsic FuncId.

Each method is a runtime intrinsic declared in pass 0 of `ssa_lower::lower`, with names like `__torajs_math_sqrt`. Backends supply the implementation:

- **LLVM AOT (Inkwell)**: a thin `f64 fn(f64) { libc_<op>(x) }` wrapper. Runs as a real call at AOT time but inlines under `-O3`'s simple-pass-manager.
- **Cranelift JIT**: a Rust `extern "C"` trampoline that defers to `f64::<op>()`. Registered as a JIT symbol via `JITBuilder::symbol`.

Both backends produce numerically equivalent output (libm and Rust's stdlib agree on these basic ops).

### Tradeoffs of the global-object model

Pros: zero overhead, no module resolution, fast to add a method.
Cons: doesn't scale. Each new namespace bloats `check.rs` and `ssa_lower.rs` with hardcoded matches. No way for users to wrap or extend stdlib types.

The escape valve is the **module system (P9)**, which lets us:
1. Move stdlib into a real `std/` directory of `.tora.ts` files
2. Have users `import { sqrt } from "std/math";`
3. Drop the hardcoded `check.rs` paths in favor of a module-resolved path

Until then, the global-object pattern is the simplest thing that ships. Don't add a 10-method namespace this way; it'll just need rewriting. Add primitives (Math, String, Number) and stop.

## How to add a stdlib method

For a new `Math.<op>(x: f64) -> f64`:

1. **`check.rs`**: extend the Math match arm:
   ```rust
   (Type::Object("Math"), m) if matches!(m, "sqrt" | "abs" | ... | "<op>") => {
       Ok(Type::Function(vec![Type::Number], Box::new(Type::Number)))
   }
   ```

2. **`ssa_lower.rs`** pass 0: declare the intrinsic and add to `Intrinsics`:
   ```rust
   let math_<op>_id = declare_intrinsic(
       &mut module,
       &mut fn_table,
       "__torajs_math_<op>",
       &[Type::F64],
       Type::F64,
   );
   ```

3. **`ssa_lower.rs`** `resolve_callee`: route `Math.<op>` → `math_<op>` FuncId; add to `is_math_unary` so coercion fires.

4. **`ssa_inkwell.rs`** Pass C: `"__torajs_math_<op>" => define_math_unary(&ctx, &llvm_module, "__torajs_math_<op>", "<libc-name>")`. Add to the `intrinsics` array.

5. **`ssa_cranelift.rs`**: Rust trampoline, register as JIT symbol.

A new method takes ~30 LOC. Binary methods follow the same pattern with `define_math_binary` and `is_math_binary`.

## Naming conventions

- Lowercase camelCase methods (`sqrt`, `parseInt`, `toString`) — matches TS.
- Constants in UPPER (`PI`, `E`, `MAX_SAFE_INTEGER`).
- Internal intrinsic names always start with `__torajs_` — never user-visible.
- Mathematical funcs accept `f64` or auto-promote integers (`Math.sqrt(2)` should just work). String funcs accept `string`.
- Conversions are explicit: no `String(123)`. Use `.toString()` once we have it.

## Error model (deferred)

We don't have a final answer here. Options under consideration:

- **Result/Option (Rust-shaped)**: `parseInt(s)` returns `Result<number, ParseError>`. Users handle with `match`. No exceptions.
- **Throw + try/catch (TS-shaped)**: traditional. Easier to write, but puts a stack-unwind machinery in the runtime.

Strong preference for the first per the no-tracing-GC design contract — exceptions imply unwinding metadata and a panic runtime. Rust's `Result` works because the type system enforces handling. Decision pending until P5 (async — async + Result interact directly).

For now, stdlib methods that can't fail panic on bad input (e.g. `Math.sqrt(-1)` returns NaN per IEEE, doesn't panic). Methods that COULD fail (`parseInt("abc")`) aren't shipped yet.

## Generic future

Currently the stdlib has no generic parameters. `Math.max(3.0, 5.0)` works, `Math.max("a", "b")` doesn't (the param type is `Type::Number`, hardcoded). When generics land:

- `function max<T: Ord>(a: T, b: T): T` — proper polymorphism
- `Array<T>.length` — works for any T
- `Map<K, V>` — explicit type params

This is a significant phase (P11+), beyond what we need for current bench cases. For the next 6-12 months the stdlib will stay monomorphic.

## What's missing for a real stdlib

Listed in rough priority:

1. **`String` methods**: `.length` ✓. `indexOf`, `slice`, `split`, `includes`, `startsWith`, `endsWith`, `toUpperCase`, `toLowerCase`, `repeat`, `charAt`, `replace`. Most need a regex/parser at some level.
2. **`Array<T>`**: the type system has `Type::Array` but no runtime. Need allocator (probably a heap header `{u64 len, u64 cap, T data[]}`), then `.push`, `.pop`, `.length`, `.map`, `.filter`, `.reduce`, indexing.
3. **`Map<K, V>` / `Set<T>`**: hash table. Wait until we have generics.
4. **`Number`**: `.toString()`, `.toFixed(n)`, `parseInt`, `parseFloat`.
5. **`Date`**: `Date.now()` for benchmarking. Wraps `clock_gettime`.
6. **`Math`** more: `random`, `sin`, `cos`, `tan`, `atan2`, `round`, `trunc`. `random` needs a seedable PRNG (Xorshift / PCG); fits the no-GC design.
7. **`process` / `Deno`-style I/O**: `process.argv`, `process.exit`, `process.stdout.write`. CLI-binary only — wasm playground would gate it out.
8. **`console`**: `error`, `warn`, `debug` — variants of `log` with stderr / level filtering.
9. **`fs`**: read/write files. CLI binary only.

Not in scope:

- `Symbol` (no GC, no proxy)
- `Proxy`, `Reflect`, `Object.defineProperty` (we're statically typed)
- `eval`, `Function` constructor (no parser at runtime)
- `Promise.all` and friends — defer until async lands
- `WeakMap` / `WeakRef` (refcount, not tracing GC; weak refs are awkward)

## Testing

Each stdlib slice should have:
- Unit test in `check.rs::tests` validating type signature
- Integration test (build + run) for each method
- A bench case if the operation has a perf signal (e.g. `Math.sqrt`-heavy algorithm)

Currently we test by running the integration source files manually. Will formalize when the test count grows past ~5 stdlib operations.
