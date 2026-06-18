// ES §13.3.3 — object destructuring with `=` default. Default fires
// when the source slot is `undefined` (null / 0 / "" / false do NOT).
// parser previously rejected `{ a = 10 }` with "expected `}` ending
// object destructuring, got Eq".
const provided: { a: number; b: string } = { a: 99, b: 'set' }
const { a = 10, b = 'def' } = provided
console.log(a)
console.log(b)
// rename + default
const o2: { x: number } = { x: 5 }
const { x: xx = 100 } = o2
console.log(xx)
// any-typed source — missing field actually undefined; default fires
const u: any = { a: 99 }
const { a: ua = 10, b: ub = 'def' } = u
console.log(ua)
console.log(ub)
// null-valued field — null !== undefined, default does NOT fire
const withNull: any = { a: null }
const { a: na = 7 } = withNull
console.log(na)
