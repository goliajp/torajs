// P1 — async method shorthand `{ async name() {} }` and async
// computed-key method `{ async [key]() {} }` per ES spec
// §15.5.4 AsyncMethod / §15.5.5 AsyncMethodComputed. Pre-fix
// tora's parser bailed at:
//   - 'expected `:` after field name `async`, got LBracket'
//     for `async [key]()` form (~2 cases under built-ins/
//     AsyncDisposableStack/* and similar).
//   - 'expected `:` after field name `async`, got Ident'
//     for `async name()` form.
//
// parser.rs parse_object_field — Ident-key form is now wired
// to real substrate (P10.3-A3b, 2026-06-03): mint a synthetic
// `__obj_async_method_<id>` Stmt::FnDecl, register it in
// `ast.async_fns` so `desugar_async` rewrites returns into
// `Promise.resolve(...)`, and emit the property value as
// `Expr::Ident(synth_name)`. As a consequence, Ident-key async
// methods now require an explicit `: Promise<T>` (or bare `: T`)
// return annotation, mirroring the top-level `async function f`
// MVP requirement at ast.rs:1834 (no return-type inference
// yet). Computed-key form (`async [Symbol.X]() {}`) stays on
// the stub-drop path — Symbol.X dispatch is a P3/P7 follow-up.

let o1 = { x: 1, async [Symbol.asyncDispose]() { } }
console.log(o1.x)                            // 1

let o2 = { y: 2, async foo(): Promise<number> { return 1 } }
console.log(o2.y)                            // 2

// Mixed regular + async methods.
let o3 = {
  z: 3,
  regular() { return "r" },
  async asyncOne(): Promise<string> { return "a" },
  async [Symbol.iterator]() { return "i" }
}
console.log(o3.z)                            // 3
