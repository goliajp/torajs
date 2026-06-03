// P10.6-A1 — `Generator.prototype.return(value)` per ES spec
// §27.5.1.7. Force-closes the generator: next() afterwards
// always returns `done: true`, and the `.return(value)` call
// itself yields `{ value, done: true }`.
//
// Narrow MVP — the spec's "abrupt completion walks through
// open `try` / `finally`" branch is unreachable today because
// J.2.b still forbids `yield` inside `try` (P10.6-A2 lifts
// that restriction). This fixture covers the happy path
// (mid-iteration cancellation of a plain yield sequence and a
// for-loop generator); finally-cleanup coverage lands when
// the J.2.b lift ships.

function* gen(): Generator<number> {
  yield 1
  yield 2
  yield 3
}
let it = gen()
console.log(it.next().value)
let r = it.return(42)
console.log(r.value)
console.log(r.done)
console.log(it.next().done)

function* counter(start: number, end: number): Generator<number> {
  for (let i = start; i < end; i++) yield i
}
let c = counter(10, 100)
console.log(c.next().value)
console.log(c.next().value)
let cr = c.return(999)
console.log(cr.value)
console.log(cr.done)
console.log(c.next().done)
