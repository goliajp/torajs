// Operators over a `number` binding that holds an `any` value, read
// from a function body. A comparison, `!`, `>>>`, a bitwise operator
// with a number side and a `+` with a string side answer their type
// from the operator, so these copies keep a scalar slot; `-n` and
// `n * 2` depend on what the binding holds and take the `any` slot.

const str: any = 'str'
const num: any = 41

const n: number = str
const b = n > 0
const s = 'x' + n
const u = n >>> 0
const z = !n
const neg = -n
const dbl = n * 2
function readTypes(): string {
  return [typeof b, typeof s, typeof u, typeof z, typeof neg].join(' ')
}
function readValues(): string {
  return [b, s, u, z, neg, dbl].join(' ')
}
console.log(readTypes())
console.log(readValues())

const k: number = num
const kb = k > 40
const ks = k + '!'
const ku = k >>> 1
const kn = -k
const ka = k & 3
const kl = k << 1
function readNumber(): string {
  return [kb, ks, ku, kn, typeof kn, ka, kl].join(' ')
}
console.log(readNumber())
