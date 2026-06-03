// P10.6-A2 — `Generator.prototype.throw(err)` per ES spec
// §27.5.1.4. Force-closes the generator and rethrows `err` to
// the caller's frame (the throw substrate's `__torajs_throw_set`
// + `emit_throw_check` carry the value through `next()` /
// `throw()`'s outer call into the surrounding try/catch).
//
// Narrow MVP — because J.2.b still forbids `yield` inside
// `try` / `catch` / `finally`, the spec's "inject at the
// suspended yield position; in-body catch observes" branch is
// unreachable today; the throw simply propagates out of the
// generator's call site. The follow-up that lifts J.2.b will
// add an in-body catch fixture once the state machine handles
// try-arms.
//
// Single-generator-class scope keeps this fixture clear of a
// separate pre-existing multi-generator dispatch issue (two
// `function*` declarations of the same yield_ty in the same
// program currently route through the wrong `__cm_<C>__next` —
// tracked as L3b "generator multi-class same-method dispatch").

function* gen(): Generator<number> {
  yield 1
  yield 2
  yield 3
}

let it = gen()
console.log(it.next().value)
try {
  it.throw("boom")
  console.log("no-throw")
} catch (e) {
  console.log("caught", e)
}
console.log(it.next().done)

let it2 = gen()
console.log(it2.next().value)
console.log(it2.next().value)
try {
  it2.throw("stop")
} catch (e) {
  console.log("caught", e)
}
console.log(it2.next().done)
