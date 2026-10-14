// A dictionary-typed binding, element or field may hold a struct cell
// that reached it through `any`; a keyed write through it lands on the
// cell, and a value an optional `number` member's slot cannot spell is
// that member's value for the typed reads.
type Opt = { a?: number; b: number; s: string }

const p: Opt = { a: 1, b: 2, s: "p" }
const anyP: any = p
const strs: Record<string, string | null>[] = [anyP]
strs[0]["a"] = null
console.log(p.a, typeof p.a, p.a ?? 9, JSON.stringify(p))
strs[0]["a"] = "x"
console.log(p.a, typeof p.a, "a" in p)
const nums: Record<string, number>[] = [anyP]
nums[0]["a"] = 2.5
nums[0]["b"] = 3
console.log(p.a, p.b, p.a + p.b)

class H {
  m: Record<string, string | null> = {}
}
const q: Opt = { a: 1, b: 2, s: "q" }
const h = new H()
;(h as any).m = q
h.m["a"] = null
console.log(q.a, typeof q.a, q.a ?? 9)
h.m["a"] = "y"
console.log(q.a, typeof q.a, JSON.stringify(q))
