// T-13.b (v0.4.0) — Symbol.for(key) global registry + keyFor(s).
// `Symbol.for('x') === Symbol.for('x')` always returns true (same
// key returns the same registered Symbol). `Symbol('x')` always
// returns a fresh handle (`!== Symbol.for('x')`). `Symbol.keyFor(s)`
// returns the registered key string for registered Symbols, or null
// (mapped from undefined) for `Symbol(...)`-fresh ones.

let a = Symbol.for('x')
let b = Symbol.for('x')
console.log(a === b)

let c = Symbol('x')
console.log(a === c)
console.log(b === c)

console.log(Symbol.keyFor(a))
console.log(Symbol.keyFor(b))

// Distinct keys → distinct registered symbols.
let d = Symbol.for('y')
console.log(Symbol.keyFor(d))
console.log(a === d)

// for() and keyFor() round-trip on same key string returns same.
let e = Symbol.for('x')
console.log(a === e)
console.log(Symbol.keyFor(e))
