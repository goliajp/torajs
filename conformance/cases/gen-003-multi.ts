// P10.6-A3 — multi-generator dispatch correctness.
//
// Two `function*` declarations with identical yield_ty + field
// shape used to collapse to the same struct sid (structural type
// interning), and ssa_lower:18693's sibling-class static
// dispatch picked the first-matching alias from a HashMap iter
// — non-deterministically routing `a().next()` to
// `__cm_<other>__next`. Same path silently dropped the
// post-call `emit_throw_check`, so `a().throw(x)` set the
// throw flag inside the callee but never jumped to the
// caller's try/catch handler.
//
// Fix: desugar_generators adds a per-class
// `__gen_nominal_<name>: number` marker field (unique field
// name → distinct struct sid → sibling-class dispatch picks
// the right `__cm_<C>__M`); ssa_lower:18693 emits
// `emit_throw_check` when the resolved callee is in
// `may_throw_fns` so `.throw` propagates the same way every
// other may-throw method already does.
//
// Acceptance: two unrelated generator classes coexist in the
// same program; their `.next()` / `.return()` / `.throw()`
// methods all route correctly and behave byte-identically to
// bun on stdout.

function* gen(): Generator<number> {
  yield 1
  yield 2
}
function* counter(): Generator<number> {
  yield 10
  yield 20
}

let it = gen()
console.log(it.next().value)
let c = counter()
console.log(c.next().value)
console.log(it.next().value)
console.log(c.next().value)

// .return correctness across both classes
let g2 = gen()
console.log(g2.next().value)
let r = g2.return(99)
console.log(r.value)
console.log(r.done)

let c2 = counter()
console.log(c2.next().value)
let cr = c2.return(88)
console.log(cr.value)
console.log(cr.done)

// .throw correctness across both classes
let g3 = gen()
console.log(g3.next().value)
try {
  g3.throw("from-gen")
} catch (e) {
  console.log("caught", e)
}

let c3 = counter()
console.log(c3.next().value)
try {
  c3.throw("from-counter")
} catch (e) {
  console.log("caught", e)
}
