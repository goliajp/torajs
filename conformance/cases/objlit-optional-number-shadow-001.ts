// An optional `number` member keeps an f64 slot; a value the slot has
// no spelling for — a null, a string, an object, a boolean — written
// through the any lane, a typed store of an any value, a literal, or a
// `null` literal, is the member's value all the same: the typed read,
// `??`, typeof, equality, arithmetic, `in`, JSON, spread and the
// any-lane read all see it, and a number stored afterwards replaces it.
type Opt = { a?: number; b: number }

function show(tag: string, p: Opt): void {
  console.log(tag, p.a, typeof p.a, p.a ?? 9, "a" in p, JSON.stringify(p))
  console.log(tag, p.a === null, p.a == null, p.a === undefined, p.a + 1, -p.a)
  const s = { ...p }
  console.log(tag, s.a, typeof s.a, JSON.stringify(s), (p as any).a)
}

// the cast spelling, the any binding, the keyed spelling
const p: Opt = { a: 1, b: 2 }
;(p as any).a = null
show("cast-null", p)
const x: any = p
x.a = "str"
show("any-str", p)
;(p as any)["a"] = { k: [1, 2] }
show("key-obj", p)
x.a = true
show("any-bool", p)
x.a = undefined
show("any-undef", p)

// a number stored through either lane replaces the shadow
x.a = 7
show("any-num", p)
x.a = null
p.a = 8
show("typed-num", p)
p.a = undefined
show("typed-undef", p)

// a typed store of an any value, and of a null literal
const v: any = null
p.a = v
show("typed-anynull", p)
const w: any = "w"
p.a = w
show("typed-anystr", p)
p.a = null
show("typed-null", p)

// the literal itself
const q: Opt = { a: v, b: 2 }
show("lit-anynull", q)
const r: Opt = { a: w, b: 3 }
show("lit-anystr", r)
const t: Opt = { a: null as any, b: 4 }
show("lit-cast", t)

// stepping and compound assignment through the any lane
const u: Opt = { a: 1, b: 2 }
;(u as any).a = "4"
;(u as any).a++
show("incr", u)
;(u as any).a += 0.5
show("compound", u)
;(u as any).a = null
;(u as any).a += 1
show("compound-null", u)

// a member the literal left out
const m: Opt = { b: 1 }
;(m as any).a = null
console.log("absent-null", m.a, typeof m.a, m.a ?? 9, "a" in m)
m.a = 3
console.log("absent-then-num", m.a, "a" in m)

// a class instance
class K {
  n?: number
  m: number = 1
  sum(): any { return (this.n ?? 0) + this.m }
}
const k = new K()
const ka: any = k
ka.n = null
console.log("class-null", k.n, typeof k.n, k.n ?? 9, "n" in k, JSON.stringify(k), k.sum())
ka.n = "two"
console.log("class-str", k.n, typeof k.n, JSON.stringify(k), k.sum())
k.n = 2
console.log("class-num", k.n, JSON.stringify(k), k.sum())

// Reflect.set reaches the same member
const rs: Opt = { a: 1, b: 2 }
console.log("reflect", Reflect.set(rs, "a", null), rs.a, JSON.stringify(rs))

// the members flow through arrays and calls
const xs: Opt[] = [p, q, r, u, m]
console.log(xs.map((o) => o.a).join("|"), xs.map((o) => typeof o.a).join("|"))
function rd(o: Opt): string { return `${o.a ?? "none"}` }
console.log(xs.map(rd).join("|"))

// JSON.parse into the annotation: a key the text lacks is undefined
function parse(s: string): string {
  const o: Opt = JSON.parse(s)
  return `${o.a} ${typeof o.a} ${o.a ?? 5} ${o.b}`
}
console.log(parse('{"b":1}'), parse('{"a":2,"b":1}'), parse('{"b":1,"a":3}'))
