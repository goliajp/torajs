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
// parser.rs parse_object_field — new branch before the regular
// computed-property path. Detect Token::Async followed by
// LBracket OR Ident-then-LParen. For computed-key form, parse
// the bracket key with the same Symbol-chain shape; for ident
// form, take the ident directly. Drop param list paren-balanced,
// drop optional return ann, drop body brace-balanced. Emit
// `Expr::Null` value under a synthetic `__async_<name>` field
// name. Same opaque-stub strategy as getter/setter / computed-
// key method shorthand. Real async method substrate (state-
// machine generation, await binding) is P-LATER.

let o1 = { x: 1, async [Symbol.asyncDispose]() { } }
console.log(o1.x)                            // 1

let o2 = { y: 2, async foo() { return 1 } }
console.log(o2.y)                            // 2

// Mixed regular + async methods.
let o3 = {
  z: 3,
  regular() { return "r" },
  async asyncOne() { return "a" },
  async [Symbol.iterator]() { return "i" }
}
console.log(o3.z)                            // 3
