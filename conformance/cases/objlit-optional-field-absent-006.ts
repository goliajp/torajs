// A literal that leaves an optional member out, then a delete or a
// mismatched store on another member: the left-out member stays absent
// and the typed store recreates the deleted one.
type P = { a?: number; b: number }
const p: P = { b: 1 }
delete (p as any).b
p.b = 3
console.log("a" in p, "b" in p, Object.keys(p), p.b, p.a)

const q: P = { b: 1 }
;(q as any).b = "s"
q.b = 3
console.log("a" in q, "b" in q, Object.keys(q), q.b, q.a, JSON.stringify(q))

const r: P = { b: 1 }
delete (r as any).b
r.a = 2
console.log("a" in r, "b" in r, Object.keys(r), r.a)
r.b = 4
console.log(Object.keys(r), JSON.stringify(r))

// a left-out member of a non-extensible object cannot be defined
const s: P = { b: 1 }
Object.preventExtensions(s)
try {
  Object.defineProperty(s, "a", { value: 1 })
} catch (err) {
  console.log("not extensible", (err as Error).constructor.name)
}
console.log("a" in s, Object.keys(s))
