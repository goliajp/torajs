// `%` over operands that are integers only at run time takes an integer
// remainder; every other number keeps the float remainder's answer,
// including the sign of a zero, whatever reads the result: an index,
// a compare, a concat, an argument, a store, a return.
const seed = Number(process.argv[2] ?? '7')
const pool = parseInt(String(seed + 5), 10)
const keys = parseInt(String(seed - 3), 10)

const xs: number[] = [
  0, -0, 1, -1, 7, -7, 12, -12, 2.5, -2.5, 0.5, Infinity, -Infinity, NaN,
  2 ** 53 - 1, 2 ** 53, 2 ** 53 + 2, -(2 ** 53), 2 ** 60, 2 ** 63, 1e300, 123456789012,
]
const ds: number[] = [
  1, 2, 3, 7, 1024, -3, -1, 0, -0, 0.5, 2.5, Infinity, -Infinity, NaN, 2 ** 53, 2 ** 53 - 1, 2 ** 60,
]

function show(r: number): string {
  return Object.is(r, -0) ? '-0' : String(r)
}

// the result returned, both operands opaque
function rem(a: number, b: number): number {
  return a % b
}

const rows: string[] = []
for (let i = 0; i < xs.length; i = i + 1) {
  const cells: string[] = []
  for (let j = 0; j < ds.length; j = j + 1) {
    cells.push(show(rem(xs[i], ds[j])))
  }
  rows.push(show(xs[i]) + ': ' + cells.join(' '))
}
console.log(rows.join('\n'))

// power-of-two and plain constant divisors over opaque dividends
const cs: string[] = []
for (let i = 0; i < xs.length; i = i + 1) {
  const x = xs[i]
  cs.push(show(x % 4096) + '/' + show(x % 2) + '/' + show(x % 1) + '/' + show(x % 1000))
}
console.log(cs.join(' '))

// index sink: a counter by a runtime pool
const arr: string[] = []
for (let j = 0; j < pool; j = j + 1) arr.push('item' + j)
let total = 0
for (let i = 0; i < 100; i = i + 1) {
  total = total + arr[i % pool].length * (i % pool)
}
console.log(total)

// compare and concat sinks: a counter by a runtime key count
const m = new Map<string, number>()
for (let i = 0; i < 50; i = i + 1) {
  const j = i % keys
  const key = i % 2 === 0 ? 'key' + j : 'k' + j
  m.set(key, (m.get(key) ?? 0) + i)
}
console.log(m.size, [...m.entries()].join(';'))

// a negative counter: the remainder of a negative multiple is -0
const negs: string[] = []
for (let i = -6; i <= 6; i = i + 1) negs.push(show(i % keys) + (i % keys === 0 ? '=' : ''))
console.log(negs.join(' '))

// argument and store sinks
function twice(x: number): number {
  return x * 2
}
const store: number[] = []
const box = { r: 0 }
for (let i = 0; i < 20; i = i + 1) {
  store.push(twice(i % pool))
  box.r = box.r + (i * 7) % pool
}
console.log(store.join(','), box.r)

// a loop-carried remainder (gcd), its divisor reaching zero
function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b
    b = a % b
    a = t
  }
  return a
}
let g = 0
for (let i = 1; i <= 60; i = i + 1) g = g + gcd(i, pool * 6)
console.log(g, gcd(seed * 1.5, 3), gcd(-12, 18), show(gcd(-0, 0)))

// an LCG through a global: dividends near 2^31, never negative
let s = seed
let acc = ''
for (let i = 0; i < 8; i = i + 1) {
  s = (s * 48271) % 2147483647
  acc = acc + (s % 26) + ','
}
console.log(acc)

// a divisor of zero or a non-integer at run time
console.log(show(seed % (keys - 4)), show(-seed % (keys - 4)), seed % (pool / 8), -seed % (keys / 8))
console.log(show(9007199254740991 % pool), show(-9007199254740991 % pool), show(2 ** 53 % keys))
