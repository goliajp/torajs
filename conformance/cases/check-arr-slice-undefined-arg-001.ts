// S213 — `arr.slice(undefined)` / `arr.slice(undefined, undefined)`
// route through ES §23.1.3.27 step 1-2 default-undefined rules:
// start=undefined → 0, end=undefined → arr.length. Pre-fix tora's
// 1-arg carve-out required Number ("Array.slice arg must be
// number, got Undefined") and the 2-arg path took the declared
// `(Number, Number)` dispatch which rejected the typed-Undefined
// operand at the strict arity gate.

const a = [1, 2, 3, 4, 5]
// Identity copies via undefined defaults.
console.log(a.slice(undefined))
console.log(a.slice(undefined, undefined))
// Mixed undefined / number.
console.log(a.slice(undefined, 3))
console.log(a.slice(1, undefined))
// Regression checks: explicit number paths unchanged.
console.log(a.slice())
console.log(a.slice(0))
console.log(a.slice(1, 3))
console.log(a.slice(-2))

// String element type — refcount path.
const s = ["a", "b", "c"]
console.log(s.slice(undefined))
console.log(s.slice(undefined, undefined))
console.log(s.slice(1, undefined))
