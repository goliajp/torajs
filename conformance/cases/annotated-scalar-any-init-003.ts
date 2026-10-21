// An un-annotated copy of a `number` binding that holds an `any` value
// holds the same value: the copy has no annotation of its own, and the
// annotation it copies has no runtime meaning. Read from a function
// body, the copy lives at the top level, and its slot must hold the
// value whatever it is, a string or a number that really is one.

const str: any = 'str'
const num: any = 41

const n: number = str
const t = n
let v = t
const w = n + 1
function readString(): string {
  return typeof t + ' ' + typeof v + ' ' + typeof w
}
console.log(readString(), t, v, w)
v = 7
console.log(readString(), v)

const k: number = num
const c = k
const d = c
function readNumber(): number {
  return c + d + 1
}
console.log(readNumber(), c, d, typeof c)

function local(x: any): string {
  const m: number = x
  const u = m
  const f = (): string => typeof u
  return f() + ' ' + u
}
console.log(local('s'), local(2))
