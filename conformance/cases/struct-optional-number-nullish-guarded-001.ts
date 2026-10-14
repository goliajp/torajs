// `r.hits ?? d` on an optional number member whose read is guarded
// (a computed-key read through a dictionary can reach a definer, so
// every field read tests the exotic bit): the nullish test runs on
// both the plain slot and an accessor's or a redefined value's answer.
type Rec = { key: string; hits?: number; n: number }

const recs: Rec[] = [
  { key: "a", n: 1 },
  { key: "b", hits: 2, n: 2 },
  { key: "c", hits: 0, n: 3 },
  { key: "a", hits: -1.5, n: 4 },
]
const counts: Record<string, number> = {}
let calls = 0
function dflt(): number {
  calls = calls + 1
  return 10
}
let total = 0
for (const r of recs) {
  counts[r.key] = (counts[r.key] ?? 0) + 1
  total = total + (r.hits ?? 1) + r.n
  console.log(r.key, r.hits ?? dflt(), r.hits ?? "none")
}
console.log(total, calls, counts["a"], counts["b"])

const label = "fallback-" + String(calls)
const g: Rec = { key: "g", n: 4 }
const answers: any[] = [null, "str", 5, undefined, 0, false]
for (const a of answers) {
  Object.defineProperty(g, "hits", { get: () => a, configurable: true })
  const v = g.hits ?? 7
  const s = g.hits ?? label
  console.log(typeof v, v, s)
}
Object.defineProperty(g, "hits", { value: null, writable: true, configurable: true })
console.log(g.hits ?? dflt(), calls)
Object.defineProperty(g, "hits", { value: 8.25, writable: true, configurable: true })
console.log(g.hits ?? dflt(), calls)
