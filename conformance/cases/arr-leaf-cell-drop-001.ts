// An array whose elements are instances of a struct with no reference
// fields is released by one kernel call instead of an inline per-slot
// walk. Each element must still get exactly the release the walk gave
// it: a shared element survives with one reference fewer, a dying one
// is freed (and its block reused), an expando bag goes with it, and a
// slot that can hold a wider subclass cell frees by the cell's size.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

function make(n: number, base: number): Tok[] {
  const out: Tok[] = []
  for (let i = 0; i < n; i = i + 1) out.push(new Tok(i % 7, base + i))
  return out
}

function sum(ts: Tok[]): number {
  let s = 0
  for (let i = 0; i < ts.length; i = i + 1) s = s + ts[i].kind * 3 + ts[i].value
  return s
}

// many releases, each over more cells than an allocator row holds
let total = 0
for (let r = 0; r < 40; r = r + 1) total = total + sum(make(700, r))
console.log(total)

// a shared element outlives the array; later allocations reuse the
// freed cells and must not land on it
const kept: Tok[] = []
for (let r = 0; r < 5; r = r + 1) {
  const ts = make(50, r * 100)
  kept.push(ts[r * 7])
}
let fresh = 0
for (let r = 0; r < 20; r = r + 1) fresh = fresh + sum(make(30, 1000))
console.log(kept.map((t) => t.kind + ':' + t.value).join(' '), fresh)

// a shared array: the second owner's release walks
let a: Tok[] = make(4, 10)
let b: Tok[] = a
a = make(1, 0)
console.log(sum(b), sum(a))
b = make(2, 5)
console.log(sum(b))

// the deque head moved by shift
const q = make(6, 20)
q.shift()
q.shift()
console.log(q.length, sum(q))

// slots never written read as holes
const sp: Tok[] = []
sp[3] = new Tok(1, 2)
console.log(sp.length, sp[3].value)

// anonymous struct cells
function pts(n: number): { x: number; y: number }[] {
  const out: { x: number; y: number }[] = []
  for (let i = 0; i < n; i = i + 1) out.push({ x: i, y: i * 2 })
  return out
}
let ps = 0
for (let r = 0; r < 30; r = r + 1) {
  const p = pts(300)
  ps = ps + p[299].y + p.length
}
console.log(ps)

// subclass cells, wider than the slot's layout
class Base {
  id: number
  constructor(id: number) {
    this.id = id
  }
}
class Wide extends Base {
  w1: number
  w2: number
  w3: number
  w4: number
  constructor(id: number) {
    super(id)
    this.w1 = id + 1
    this.w2 = id + 2
    this.w3 = id + 3
    this.w4 = id + 4
  }
}
let ws = 0
for (let r = 0; r < 30; r = r + 1) {
  const bs: Base[] = []
  for (let i = 0; i < 40; i = i + 1) bs.push(i % 2 === 0 ? new Base(i) : new Wide(i))
  ws = ws + bs[39].id + (bs[39] as Wide).w4
}
console.log(ws)

// expando bags on elements, one of them shared (a class of its own:
// a cell that crosses into `any` frees by what it says of itself)
class Bag {
  n: number
  constructor(n: number) {
    this.n = n
  }
}
let bagKept: Bag = new Bag(0)
for (let r = 0; r < 10; r = r + 1) {
  const bs: Bag[] = []
  for (let i = 0; i < 8; i = i + 1) bs.push(new Bag(r + i))
  ;(bs[2] as any).extra = 'bag' + r
  ;(bs[5] as any).extra = r * 10
  bagKept = bs[5]
}
console.log(bagKept.n, (bagKept as any).extra)
