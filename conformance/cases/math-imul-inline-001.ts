// Math.imul at every operand shape: f64 edge values (ToInt32 of NaN,
// ±Infinity, beyond 2^53, fractions, negatives), integer-typed
// operands from loop counters and `| 0`, booleans, and any-typed
// operands that go through ToNumber first.

function im(a: number, b: number): number {
  return Math.imul(a, b)
}

const edge: number[] = [
  0,
  -0,
  1,
  -1,
  3.7,
  -3.7,
  2147483647,
  -2147483648,
  2147483648,
  4294967295,
  4294967296 + 3,
  9007199254740993,
  -9007199254740993,
  1e21,
  -1e300,
  NaN,
  Infinity,
  -Infinity,
  0.5,
  65537,
]

let acc = 0
for (let i = 0; i < edge.length; i++) {
  const row: number[] = []
  for (let j = 0; j < edge.length; j++) {
    const r = im(edge[i], edge[j])
    row.push(r)
    acc = (acc + r) | 0
  }
  console.log(i, row.join(','))
}
console.log('acc', acc)

let h = 0
for (let k = 0; k < 1000; k++) {
  h = Math.imul(h ^ k, 0x5bd1e995)
  h = h ^ (h >>> 15)
}
console.log('hash', h)

let x = 123456789 | 0
let y = -987654321 | 0
console.log('int', Math.imul(x, y), Math.imul(x, x), Math.imul(y, y))

const t: any = true
const f: any = false
console.log('bool', Math.imul(t, 7), Math.imul(f, 7), Math.imul(t, t))

const s: any = '65537'
const o: any = { valueOf: () => 65537 }
const u: any = undefined
const bad: any = 'x'
console.log('any', Math.imul(s, o), Math.imul(o, -3), Math.imul(u, 5), Math.imul(bad, 2))

const big = 2 ** 40 + 5
console.log('wide', Math.imul(big, 3), Math.imul(-big, 3), Math.imul(big, big))
console.log('const', Math.imul(0xffffffff, 5), Math.imul(-2147483648, -1), Math.imul(3, 4))
