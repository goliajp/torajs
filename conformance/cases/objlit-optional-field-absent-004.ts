// A literal that leaves an optional member out builds a cell that lacks
// it on every own-property face, and a store creates it.
type O = { a?: number; b: number; c?: string }

const p: O = { b: 2 }
console.log("a" in p, "b" in p, Object.hasOwn(p, "a"), p.hasOwnProperty("c"))
console.log(p.propertyIsEnumerable("a"), Reflect.has(p, "a"))
console.log(Object.keys(p), JSON.stringify(p))
console.log(Object.getOwnPropertyNames(p), Reflect.ownKeys(p))
console.log(Object.entries(p), Object.values(p))
console.log(Object.getOwnPropertyDescriptor(p, "a"))
console.log(Object.getOwnPropertyDescriptors(p))
console.log(p)
for (const k in p) console.log("key", k)

// a typed store creates the member
const q: O = { b: 3 }
q.c = "x"
console.log("c" in q, q.c, "a" in q, JSON.stringify(q))
q.a = 1
console.log("a" in q, q.a)

// an any-lane store creates it too
const r: O = { b: 4 }
const ra: any = r
ra.a = 7
console.log("a" in r, r.a, Object.hasOwn(r, "a"))

// a compound store reads undefined first
const s: O = { b: 5 }
s.a ??= 9
console.log(s.a, "a" in s)

// an integrity level keeps the member out
const f: O = { b: 6 }
Object.freeze(f)
try {
  ;(f as any).a = 1
} catch (e) {
  console.log("frozen", (e as Error).constructor.name)
}
console.log("a" in f, Object.keys(f), Object.isFrozen(f))
const n: O = { b: 7 }
Object.preventExtensions(n)
try {
  n.a = 1
} catch (e) {
  console.log("non-extensible", (e as Error).constructor.name)
}
console.log("a" in n, Object.keys(n), Object.isExtensible(n))
const sealed: O = { b: 8 }
Object.seal(sealed)
console.log(Object.isSealed(sealed), "a" in sealed)

// a define creates it with the attributes it names
const g: O = { b: 9 }
Object.defineProperty(g, "a", { value: 7, enumerable: false, writable: true, configurable: true })
console.log("a" in g, g.a, Object.keys(g), Object.getOwnPropertyDescriptor(g, "a"))

// a delete of a missing member succeeds
const d: O = { b: 10 }
console.log(delete (d as any).a, "a" in d)

// the cells a function hands back, and the ones an array holds
function mk(i: number): O {
  return i % 2 ? { b: i } : { a: i, b: i }
}
let created = 0
let total = 0
for (let i = 0; i < 8; i++) {
  const o = mk(i)
  if (!("a" in o)) {
    o.a = 100
    created++
  }
  total += o.a ?? 0
}
console.log(created, total)
const arr: O[] = [{ b: 1 }, { a: 2, b: 3 }, { b: 4, c: "z" }]
for (const o of arr) console.log(JSON.stringify(o), "a" in o, "c" in o)
console.log(arr)
const nested: { o: O; n: number } = { o: { b: 1 }, n: 2 }
console.log(nested, JSON.stringify(nested), "a" in nested.o)
