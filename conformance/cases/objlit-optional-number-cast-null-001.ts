// An optional `number` member keeps an f64 slot. A value cast from the
// any lane into the annotated layout may carry `null` for it, or lack
// the key: the `null` lives in the field's sidecar and reads back as
// null through every face, and the lacking key is no own property.
type Opt = { a?: number; b: number }

function show(tag: string, o: Opt): void {
  console.log(tag, o.a, typeof o.a, o.a === null, o.a ?? 9, "a" in o, JSON.stringify(o))
  console.log(tag, Object.keys(o).join(","), Object.values(o).join("|"), Object.entries(o).join(";"))
  const s = { ...o }
  console.log(tag, s.a, typeof s.a, "a" in s, JSON.stringify(s))
}

// an annotated binding
show("null", JSON.parse('{"a":null,"b":1}'))
show("lack", JSON.parse('{"b":2}'))
show("num", JSON.parse('{"a":3,"b":4}'))
const src: any = { a: undefined, b: 7 }
const u: Opt = src
show("undef", u)

// an array of the layout, element by element
const xs: Opt[] = JSON.parse('[{"a":null,"b":1},{"b":2},{"a":3,"b":4}]')
console.log(xs.map((o) => o.a).join("|"), xs.map((o) => typeof o.a).join("|"), xs.map((o) => "a" in o).join("|"))

// a function the any lane calls casts its parameter
function f(o: Opt): string {
  return `${o.a} ${typeof o.a} ${"a" in o}`
}
const g: any = f
console.log(g({ a: null, b: 1 }), g({ b: 2 }), g({ a: 3, b: 4 }))

// a return of an any value, a push
function parse(s: string): Opt {
  const v: any = JSON.parse(s)
  return v
}
show("ret", parse('{"a":null,"b":8}'))
const anyv: any = { a: null, b: 9 }
const ys: Opt[] = []
ys.push(anyv)
show("push", ys[0])

// Object.assign copies the null across
const t: Opt = { a: 1, b: 2 }
Object.assign(t, ys[0])
show("assign", t)

// a class with an optional number member, cast from the any lane
class K {
  n?: number
  m: number = 1
}
const kv: any = { n: null, m: 2 }
const k: K = kv
console.log("class", k.n, typeof k.n, k.n === null, "n" in k, JSON.stringify(k))

// a number stored afterwards replaces the null
const w: Opt = JSON.parse('{"a":null,"b":1}')
w.a = 5
show("replaced", w)
