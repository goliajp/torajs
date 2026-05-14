// P1 — async arrow function expressions per ES spec §15.8.6
// AsyncArrowFunctionExpression. Pre-fix tora's parser bailed at
// 'expected expression, got Async' on the `async x => ...` /
// `async (...) => ...` shapes. Test262 uses these for
// AsyncDisposableStack-style fixtures (~5+ direct cases plus
// downstream broader sample).
//
// Async function* (async generator function expression) is also
// accepted (5 more cases).
//
// Implementation: parse_primary detects Token::Async followed by
//   - Ident => ...      (single-param shorthand arrow)
//   - (...) => ...      (parenthesized arrow)
//   - function ...      (regular async fn — handled by existing branch)
//   - function* ...     (async generator — same branch + accept Star)
// For the arrow forms, drop the param list / Ident, drop the
// FatArrow, drop the body (block-balanced or expr.parse_assign),
// emit empty placeholder Expr::ArrowFn — same opaque-stub
// strategy as generator-expression / getter-setter / computed-key
// method bodies. Real async-arrow substrate (state-machine
// generation, await binding) is P-LATER.

let f = async x => x
let g = async _ => {}
let h = async (a, b) => a + b
let i = async () => { return 42 }
let j = async function*() { yield 1 }
let k = async function* named() { yield 2 }
console.log("parse-ok")                       // parse-ok

// Async arrow as RHS of assignment-style let.
let m = async (x, y, z) => { return x + y + z }
console.log("m-ok")                           // m-ok

// Multi-line block body.
let n = async (x) => {
  let r = x * 2
  return r
}
console.log("n-ok")                           // n-ok
