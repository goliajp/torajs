// Two layouts share a member name: `a` is an optional `number` member
// of one and a required `number` of the others. A write that shadows
// the optional one (a null cast in, a string through Reflect.set)
// leaves the required ones' typed reads plain f64 loads.
type Opt = { a?: number; b: number }
type Req = { a: number; c: string }
class C {
  a: number = 10
  d: string = "d"
}

const p: Opt = JSON.parse('{"a":null,"b":2}')
const r: Req = { a: 5, c: "x" }
const c = new C()
let s = 0
for (let i = 0; i < 1000; i++) {
  s += r.a + c.a
}
console.log(p.a, typeof p.a, r.a, c.a, s, typeof r.a, r.a + 1, Math.max(r.a, c.a))
console.log(JSON.stringify(r), Object.values(r).join(","), Object.values(c).join(","), JSON.stringify(p))
r.a = 7
c.a = 8
console.log(r.a * 2, c.a * 2, [r, r].map((o) => o.a).join(","), [c].map((o) => o.a + 0.5).join(","))

const q: Opt = { a: 3, b: 4 }
console.log(Reflect.set(q, "a", "s"), q.a, typeof q.a, r.a, c.a, p.a ?? r.a)
const rs: Req[] = [r, { a: 1.5, c: "y" }]
console.log(rs.map((o) => o.a * 2).join(","), rs.reduce((acc, o) => acc + o.a, 0))
