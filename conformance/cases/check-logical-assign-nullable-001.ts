// ES §13.13 — `||=` / `&&=` desugar to `lhs = lhs || rhs` / `lhs = lhs
// && rhs`. When `lhs: Nullable<T>` and `rhs: T` the operand types
// don't match in the strict sense, but the union is well-defined: T
// for `||` (all return paths land in T), Nullable<T> for `&&` (null
// short-circuit preserves the shape). tora previously rejected
// `s: string | null ||= 'def'` with "require matching operand types".
let s: string | null = null
s ||= 'def'
console.log(s)

let n: number | null = 0
n ||= 99
console.log(n)

const obj: { x: number | null; y: string } = { x: null, y: 'a' }
obj.x ??= 7
console.log(obj.x)

let arr: number[] | null = null
arr ||= [1, 2, 3]
console.log(arr)
arr ||= [9]
console.log(arr)

let s2: string | null = 'hi'
s2 &&= 'replaced'
console.log(s2)

let s3: string | null = null
s3 &&= 'never'
console.log(s3)
