// P10.7 — Default-Any generator. When the user omits the return-
// type annotation (`function* foo() {...}`), torajs infers
// `Generator<any>` so the body's `yield` values flow through the
// NaN-box AnyValue tier via `Expr::As { …, ty_ann: "any" }` wrap
// inside `GenSm::emit_yield_return` + the new `Expr::As`
// widen-to-Any path in `ssa_lower:26122` (the variant doc-
// comment had long promised this widening; it was previously
// identity-only, so any non-let-decl Any assignment site —
// including ObjectLit field writes — silently dropped the box).
//
// Acceptance: an untyped generator yielding mixed-type primitives
// (number / string / boolean) reads back byte-equal to bun.
//
// Async-side default-any (`async function foo() { return e }`)
// follows in a separate P10.7 sub-step — it needs `Promise<any>
// .then(...)` typecheck support, which is wider than the current
// inner-T whitelist (Number / String / Boolean only).

function* gen() {
  yield 1
  yield "hello"
  yield true
}
let it = gen()
console.log(it.next().value)
console.log(it.next().value)
console.log(it.next().value)
console.log(it.next().done)

// Explicit `Generator<any>` annotation must keep working.
function* explicitAny(): Generator<any> {
  yield 42
  yield "explicit"
}
let ea = explicitAny()
console.log(ea.next().value)
console.log(ea.next().value)

// Explicit primitive T stays a primitive-typed step (regression
// guard for the existing path).
function* primT(): Generator<number> {
  yield 7
  yield 8
}
let pt = primT()
console.log(pt.next().value)
console.log(pt.next().value)
console.log(pt.next().done)
