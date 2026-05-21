// P10.4 — `await e` on non-Promise primitive (Number / String /
// Boolean). Per ES spec, conceptually `Promise.resolve(e)` is
// constructed and its resolved value is yielded — for a primitive
// e that collapses to e itself. Pre-fix tora errored at typecheck
// "no member .value on type Number" because the parser desugars
// `await e` to `e.value` Member access and only Promise<T>.value
// had a typecheck arm. Now both check.rs and ssa_lower carry a
// matching `.value`-on-primitive identity arm.

// Number
let n = await 42
console.log('num', n)
let n2 = await (10 + 20)
console.log('num-expr', n2)

// String
let s = await 'hello'
console.log('str', s)

// Boolean
let b = await true
console.log('bool-t', b)
let b2 = await false
console.log('bool-f', b2)

// Mixed: real Promise<T> still awaits normally (regression guard)
let p: Promise<number> = Promise.resolve(99)
let v = await p
console.log('real-promise', v)
