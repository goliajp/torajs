// int32 results of `| 0` and Math.imul read every way a value can be:
// back to a number, stored, as an index, compared with a wider value,
// passed to a function, printed, and fed on through more arithmetic.
// The inputs come from argv so nothing folds; negatives, -2^31 and
// values past 2^31 show an upper half the int32 was not extended into.
const raw = (
  process.argv[2] ?? '-1,-2147483648,2147483647,1e20,NaN,-0,3,-7,4294967295,2147483648,65537,-3.7'
).split(',')
const xs: number[] = []
for (const s of raw) xs.push(Number(s))

function orZero(a: number): number {
  return a | 0
}
function imul(a: number, b: number): number {
  return Math.imul(a, b)
}
function neg(a: number): number {
  return -a | 0
}
function plus(a: number, b: number): number {
  return (a + b) | 0
}
function minus(a: number, b: number): number {
  return (a - b) | 0
}
function id(x: number): number {
  return x
}

const table: number[] = [10, 11, 12, 13, 14, 15, 16, 17]
const wide = Number(process.argv[3] ?? '4294967295')
const kept: number[] = []
let acc = 0
for (let i = 0; i < xs.length; i++) {
  const a = xs[i]
  const b = xs[(i + 3) % xs.length]
  const r = a | 0
  const m = Math.imul(a, b)
  kept.push(r)
  kept.push(m)
  console.log(orZero(a), imul(a, b), neg(a), plus(a, b), minus(a, b))
  console.log(r, m, kept[kept.length - 2], kept[kept.length - 1])
  console.log(table[r], table[m], table[r & 7], table[m & 7])
  console.log(r < wide, m > -wide, r === -1, m < 0, r == wide, m <= -2147483648)
  console.log(id(r), id(m), r * 2, m + 0.5, r / 2, m % 5)
  console.log((r + m) | 0, Math.imul(r, m) | 0, (m << 3) >> 1, (r ^ m) >>> 0)
  console.log(Object.is(r, -0), Object.is(m, -0), 1 / (r || 1), `${r}:${m}`)
  acc = (acc + Math.imul(a, b)) | 0
}
console.log(acc, kept.length, kept.join(' '))
