// A binding that reads a field, then outlives an overwrite of that
// field, must still name the old value: return it, keep it, pass it,
// print it, or store it into another field.
class N {
  a: number
  b: number
  c: number
  d: number
  constructor(n: number) {
    this.a = n
    this.b = n
    this.c = n
    this.d = n
  }
}
type P = { k: N; j: N }

function swap(p: P, n: number): N {
  const old = p.k
  p.k = new N(n)
  return old
}

function setK(p: P, n: number): void {
  p.k = new N(n)
}

function swapViaCall(p: P, n: number): N {
  const old = p.k
  setK(p, n)
  return old
}

function sum(x: N): number {
  return x.a + x.b + x.c + x.d
}

class Holder {
  k: N
  constructor() {
    this.k = new N(0)
  }
  replace(n: number): N {
    const old = this.k
    this.k = new N(n)
    return old
  }
}

const p: P = { k: new N(0), j: new N(-1) }
let total = 0

// result discarded
for (let i = 1; i <= 1000; i++) swap(p, i)
// result kept in a local
for (let i = 1; i <= 1000; i++) {
  const o = swap(p, i)
  total += o.a
}
// result stored into an array
const kept: N[] = []
for (let i = 1; i <= 1000; i++) kept.push(swap(p, i))
// the overwrite happens inside a callee
for (let i = 1; i <= 1000; i++) kept.push(swapViaCall(p, i))
// through `this`
const h = new Holder()
for (let i = 1; i <= 1000; i++) kept.push(h.replace(i))

// passed on, printed, and moved into a sibling field after the overwrite
for (let i = 1; i <= 1000; i++) {
  const o = p.k
  p.k = new N(i * 3)
  total += sum(o)
  if (i % 250 === 0) console.log(o.a, o.d)
  const q = p.k
  p.k = new N(i * 7)
  p.j = q
}

const junk: N[] = []
for (let i = 0; i < 4000; i++) junk.push(new N(100000 + i))
for (const x of kept) total += x.a + x.b
console.log(total, kept.length, p.k.a, p.j.a, h.k.a, junk.length)
