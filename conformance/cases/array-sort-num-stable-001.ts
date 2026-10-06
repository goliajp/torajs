// `(a, b) => a - b` / `b - a` over number elements: every length
// around the small-run and merge boundaries, presorted, reversed and
// all-equal input, -0 / +0 ties (the sort is stable, so they keep
// their order), infinities and NaN; plus records sorted by a number
// key, where stability is visible through the record's position.
function show(xs: number[]): string {
  const out: string[] = []
  for (let i = 0; i < xs.length; i = i + 1) {
    const x = xs[i]
    out.push(Object.is(x, -0) ? "-0" : String(x))
  }
  return out.join(",")
}

function digest(xs: number[]): string {
  const s = show(xs)
  let h = 0
  for (let i = 0; i < s.length; i = i + 1) h = (h * 31 + s.charCodeAt(i)) % 1000000007
  let sorted = true
  for (let i = 1; i < xs.length; i = i + 1) if (xs[i - 1] > xs[i]) sorted = false
  return xs.length + " " + sorted + " " + h
}

function print(label: string, xs: number[]): void {
  console.log(label, xs.length <= 40 ? show(xs) : digest(xs))
}

let seed = 12345
function next(): number {
  seed = (seed * 48271) % 2147483647
  return seed
}

const lens: number[] = [0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 64, 65, 1000]
for (let li = 0; li < lens.length; li = li + 1) {
  const n = lens[li]
  const ints: number[] = []
  const floats: number[] = []
  const zeros: number[] = []
  for (let i = 0; i < n; i = i + 1) {
    ints.push(next() % 100)
    floats.push((next() % 2001) / 8 - 125)
    const r = next() % 6
    zeros.push(r === 0 ? -0 : r === 1 ? 0 : r === 2 ? Infinity : r === 3 ? -Infinity : r - 4.5)
  }
  const a = ints.slice()
  a.sort((x, y) => x - y)
  print("int asc " + n, a)
  a.sort((x, y) => x - y)
  print("int again " + n, a)
  a.reverse()
  a.sort((x, y) => x - y)
  print("int rev " + n, a)
  const d = ints.slice()
  d.sort((x, y) => y - x)
  print("int desc " + n, d)
  const f = floats.slice()
  f.sort((x, y) => x - y)
  print("float asc " + n, f)
  const fd = floats.slice()
  fd.sort((x, y) => y - x)
  print("float desc " + n, fd)
  const z = zeros.slice()
  z.sort((x, y) => x - y)
  print("zeros asc " + n, z)
  const zd = zeros.slice()
  zd.sort((x, y) => y - x)
  print("zeros desc " + n, zd)
  const same: number[] = []
  for (let i = 0; i < n; i = i + 1) same.push(i % 2 === 0 ? 0 : -0)
  same.sort((x, y) => x - y)
  print("same " + n, same)
}

const big: number[] = []
for (let i = 0; i < 1000; i = i + 1) big.push(i % 3 === 0 ? -0 : i % 3 === 1 ? 0 : (i % 7) - 3)
big.sort((x, y) => x - y)
let zs = ""
for (let i = 0; i < big.length; i = i + 1) if (big[i] === 0) zs = zs + (Object.is(big[i], -0) ? "m" : "p")
console.log(zs.length, zs.slice(0, 40), zs.slice(zs.length - 40))

const withNan: number[] = [3, NaN, 1, 2]
withNan.sort((x, y) => x - y)
console.log(show(withNan))
const nanOnly: number[] = [NaN, NaN]
nanOnly.sort((x, y) => x - y)
console.log(show(nanOnly))

class Rec {
  key: number
  id: number
  constructor(key: number, id: number) {
    this.key = key
    this.id = id
  }
}
const recLens: number[] = [2, 31, 32, 33, 1000]
for (let li = 0; li < recLens.length; li = li + 1) {
  const n = recLens[li]
  const recs: Rec[] = []
  for (let i = 0; i < n; i = i + 1) recs.push(new Rec(next() % 5, i))
  recs.sort((p, q) => p.key - q.key)
  let stable = true
  for (let i = 1; i < n; i = i + 1) {
    const p = recs[i - 1]
    const q = recs[i]
    if (p.key > q.key || (p.key === q.key && p.id > q.id)) stable = false
  }
  console.log("recs " + n, stable, recs[0].key, recs[0].id, recs[n - 1].key, recs[n - 1].id)
}
