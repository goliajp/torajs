// P10.7 async-side — Default-Any async fn.
//
// `async function foo() { return e }` without an explicit return
// annotation desugars to `Promise<any>` via three coordinated
// pieces:
//   1. `ast::desugar_async` defaults `declared_ty` to `"any"`
//      when the annotation is missing, then wraps each return
//      value in `Expr::As { …, ty_ann: "any" }` so the
//      synthesised `Promise.resolve(...)` arg lowers as
//      AnyValue (NaN-box at SSA).
//   2. `check::process_on`-style sidekick — `check/promise_static.rs`
//      accepts `Type::Any` on `Promise.resolve`, surfacing
//      `Promise<Any>` instead of failing the v0.5 MVP whitelist.
//   3. `check.rs:5008` — `.then` / `.catch` accept `Promise<Any>`
//      and a `(Any) => R` cb (R is unrestricted: Number /
//      String / Boolean / Void / Any), yielding `Promise<R>`.
//
// Acceptance: an untyped async fn returning a primitive reads
// through `.then` and surfaces byte-identical stdout to bun.
// Heterogeneous yields ought to round-trip the AnyValue tier.

async function foo() {
  return 42
}
foo().then((v: any) => console.log("got", v))

async function bar() {
  return "hello"
}
bar().then((v: any) => console.log("got", v))

async function baz() {
  return true
}
baz().then((v: any) => console.log("got", v))
