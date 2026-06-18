// ES §13.4.2 - `lhs ?? rhs` on a non-nullable lhs is a static no-op:
// lhs is never null/undefined, so the result is always lhs and rhs
// is never evaluated. bun and spec both accept the construct; pre-
// fix tr rejected with "`??` left operand must be nullable, got
// {ty}" unless rhs was Any.

console.log(0 ?? 'default')
console.log('' ?? 'default')
console.log(false ?? 'default')
console.log(1 ?? 'default')
console.log('hi' ?? 'default')
console.log(true ?? 'default')

// nested
const a: number = 0
console.log(a ?? 99)

const b: string = ''
console.log(b ?? 'fallback')

// rhs not evaluated when lhs is non-nullable. If rhs had a side
// effect it would not fire — but a pure constant is the easiest
// regression check.
const c: number = 42
console.log(c ?? 'not-reached')
