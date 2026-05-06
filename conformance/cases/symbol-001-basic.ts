// T-13.a (v0.4.0) — Symbol value type basics. Each Symbol(desc?)
// call allocates a fresh heap block; identity is pointer identity
// (`Symbol('x') === Symbol('x')` is false; same Symbol via the
// same binding is true). console.log formats `Symbol(<desc>)`,
// `Symbol()` when desc is omitted.

let s1 = Symbol('hello')
let s2 = Symbol('hello')
console.log(s1 === s2)
console.log(s1 === s1)
console.log(s2 === s2)
console.log(s1 !== s2)

// Description shows up in console output.
console.log(s1)
console.log(s2)

// No-desc form prints `Symbol()`.
let s3 = Symbol()
console.log(s3)

// typeof — should be "symbol" matching the JS spec.
console.log(typeof s1)
console.log(typeof s3)
