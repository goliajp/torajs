// A required member written a value of another type through the any
// lane holds that value: the typed reads answer it (§10.1.8.1) and
// their consumers take the any lane — printing, typeof, equality,
// arithmetic, templates, JSON, spread, Object.values, a binding, a
// field, `++` and `+=` — where the read used to answer
// the stale slot, raw bits, or a TypeError.
type P = { a?: number; b: number; s: string; f: boolean }

const p: P = { a: 1, b: 2, s: "x", f: true }
const pa: any = p
pa.b = "str"
pa.s = 7
pa.f = null
console.log(p.b, p.s, p.f, typeof p.b, typeof p.s, typeof p.f)
console.log(p.b + 1, `${p.b}!`, p.b === ("str" as any), p.b ?? 0, p.b ? "t" : "f")
console.log(JSON.stringify(p), Object.values(p).join("|"))
const q = { ...p }
console.log(q.b, q.s, q.f, JSON.stringify(q))
const { b } = p
const m = p.b
const o = { k: p.b }
let v = 0
v = p.b
console.log(b, m, o.k, v, typeof m, typeof v)
pa.b = "4"
p.b++
console.log(p.b, typeof p.b)
pa.b = "4"
p.b += 1
console.log(p.b, typeof p.b)
pa.b = 2.5
console.log(p.b, p.b * 2, typeof p.b)

// a class member, read inside its own method and outside
class K {
  n: number = 1
  t: string = "t"
  show(): string {
    return `${this.n}:${typeof this.n}:${this.t}`
  }
  setN(v: number): void {
    this.n = v
  }
}
const k = new K()
;(k as any).n = "two"
;(k as any).t = 3
console.log(k.show(), k.n, k.t, JSON.stringify(k))
// a method called through `any` runs its twin, whose `this.n = v`
// carries the caller's value
const ka: any = k
ka.setN(5)
console.log(k.n, typeof k.n)
ka.setN("five")
console.log(k.n, typeof k.n, k.show())
const ks: K[] = [k, new K()]
console.log(ks.map((x) => x.n).join(","), ks.map((x) => typeof x.n).join(","))
