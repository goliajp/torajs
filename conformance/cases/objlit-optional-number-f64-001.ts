// An optional `number` member keeps an f64 slot: a member a literal
// leaves out, or spells `undefined`, reads as undefined, never as 0 —
// through arithmetic (NaN), `??`, `in`, typeof, equality, printing,
// JSON, spread, keys and delete.
type Opt = { a?: number; b: number }

function make(i: number): Opt {
  if (i % 4 === 0) return { a: i, b: i }
  return { b: i }
}

const o = make(1)
console.log(o.a, typeof o.a, o.a === undefined, o.a == null, o.a === null)
console.log(o.a * 2, o.a + 1, -o.a, Math.abs(o.a), `${o.a}`, Number(o.a))
console.log(o.a ?? 0, (o.a ?? 5) * 2, o.a || 7, !o.a, o.a ? "t" : "f")
console.log("a" in o, Object.hasOwn(o, "a"), Object.keys(o), JSON.stringify(o))
console.log(o, [o], { o })

const p = make(4)
console.log(p.a, typeof p.a, p.a === undefined, p.a * 2, p.a ?? 0, "a" in p)
console.log(Object.keys(p), JSON.stringify(p), p)

// a store creates the member; a later `undefined` keeps it
const q: Opt = { b: 2 }
q.a = 3
console.log(q.a, q.a + 1, "a" in q, Object.keys(q).length)
q.a = undefined
console.log(q.a, typeof q.a, "a" in q, Object.keys(q).length, JSON.stringify(q))
q.a ??= 9
console.log(q.a, "a" in q)

// spelled `undefined` is an own property
const u: Opt = { a: undefined, b: 1 }
console.log(u.a, "a" in u, Object.keys(u), JSON.stringify(u), u)

// delete takes it away
const d: Opt = { a: 1, b: 2 }
delete (d as any).a
console.log("a" in d, Object.keys(d), JSON.stringify(d))
d.a = 5
console.log(d.a, "a" in d, Object.keys(d).length)

// spread copies only what the source owns
const s1 = { ...make(1) }
const s2 = { ...make(4) }
const s3 = { ...u }
console.log("a" in s1, s1.a, "a" in s2, s2.a, "a" in s3, s3.a)
console.log(JSON.stringify(s1), JSON.stringify(s2), JSON.stringify(s3))
const { a = 11, b } = make(1)
console.log(a, b)

// the member flows through bindings, arrays and calls
const v = o.a
console.log(v, typeof v, v === undefined)
const xs: Opt[] = [make(1), make(4), { a: undefined, b: 9 }]
console.log(xs.map((x) => x.a).join(","), xs.map((x) => "a" in x).join(","))
console.log(xs.map((x) => x.a ?? -1).reduce((m, n) => m + n, 0))
function rd(x: Opt): number { return x.a ?? 100 }
function pass(n: number | undefined) { return n === undefined ? "u" : n * 2 }
console.log(rd(o), rd(p), pass(o.a), pass(p.a))
const anyv: any = o.a
console.log(anyv, anyv === undefined, typeof anyv)

// classes, generic and inline spellings
class K {
  n?: number
  m: number = 1
}
const k = new K()
console.log(k.n, typeof k.n, k.n * 2, k.n ?? 4, "n" in k, JSON.stringify(k))
k.n = 2
console.log(k.n, k.n * 2, JSON.stringify(k))
type G<T> = { v?: T; w: number }
const g: G<number> = { w: 1 }
console.log(g.v, g.v ?? 6, "v" in g, JSON.stringify(g))
function inl(x: { c?: number; d: number }): string { return `${x.c} ${x.c ?? 0} ${"c" in x}` }
console.log(inl({ d: 1 }), inl({ c: 2, d: 1 }))

// `in` asks the prototype chain for a member the cell lacks
console.log("a" in make(1), "toString" in make(1), "zz" in make(1))
;(Object.prototype as any).a = 1
console.log("a" in make(1), "a" in make(4), "a" in { b: 1 })
delete (Object.prototype as any).a
console.log("a" in make(1))

// the hot loop
let total = 0
let created = 0
for (let i = 0; i < 40; i = i + 1) {
  const w = make(i)
  if (!("a" in w)) {
    w.a = 2
    created++
  }
  w.b = w.b + 1
  total = total + (w.a ?? 0) + w.b
}
console.log(total, created)
