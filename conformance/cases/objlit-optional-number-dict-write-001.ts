// A `number` written through a dictionary, a computed key, Reflect.set
// or Object.assign fits an f64 slot: a typed read of an optional
// `number` member keeps its load, and the write shadows only slots of
// other types.
type Opt = { a?: number; b: number; s: string }

const counts: Record<string, number> = {}
for (const w of ["x", "y", "x", "z", "x"]) {
  counts[w] = (counts[w] ?? 0) + 1
}
const p: Opt = { a: 1, b: 2, s: "p" }
const q: Opt = { b: 3, s: "q" }
console.log(counts["x"], counts["y"], counts["z"], p.a, q.a, (p.a ?? 0) + (q.a ?? 0), p.s, q.s)

const anyo: any = { s: "t", b: 1 }
anyo["b"] = 2.5
const k = "s"
anyo[k] = 42
console.log(anyo.b, anyo.s, p.s, q.s, p.a, q.a ?? 7)

const t: Opt = { a: 4, b: 5, s: "t" }
const from: Opt = { a: 6, b: 7, s: "f" }
Object.assign(t, from, { b: 7 })
console.log(t.a, t.b, t.s, Reflect.set(t, "a", 8), t.a, Reflect.set(t, k, "u"), t.s)
const key: string = "a"
Reflect.set(t, key, 9.5)
console.log(t.a, typeof t.a, "a" in t, JSON.stringify(t))

// a dictionary counting pass next to typed reads, the bench's shape
type Rec = { key: string; hits?: number; n: number }
const recs: Rec[] = [{ key: "k0", n: 0 }, { key: "k1", hits: 1, n: 1 }, { key: "k2", hits: 2, n: 2 }]
const tally: Record<string, number> = {}
let total = 0
for (let i = 0; i < 9; i++) {
  const r = recs[i % 3]
  tally[r.key] = (tally[r.key] ?? 0) + 1
  total = total + (r.hits ?? 1) + r.n
}
console.log(total, tally["k0"], tally["k1"], tally["k2"], recs[0].hits, recs[1].hits, typeof recs[0].hits)
