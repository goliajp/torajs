// A computed read on a dictionary can answer an inherited
// `Object.prototype` member, `__defineGetter__` among them. A read whose
// value only meets `??`, an arithmetic operator, a comparison or
// `typeof` never hands that function to a call, so the fields of the
// program's records keep their plain reads; the values are still the
// ones the spec gives, the inherited function converted where an
// operator converts it.

type Rec = { key: string; hits?: number; n: number }
const recs: Rec[] = []
for (let i = 0; i < 6; i = i + 1) {
  if (i % 2 === 0) {
    recs.push({ key: 'k' + i, n: i })
  } else {
    recs.push({ key: 'k' + i, hits: i * 1.5, n: i })
  }
}
const counts: Record<string, number> = {}
const keys = ['k1', '__define' + 'Getter__', 'toString', 'k1', 'constructor', 'hasOwnProperty']
let total = 0
for (let i = 0; i < recs.length; i = i + 1) {
  const r = recs[i]
  counts[r.key] = (counts[r.key] ?? 0) + 1
  total = total + (r.hits ?? 1) + r.n
}
console.log(total, Object.keys(counts).length)

const seen: string[] = []
for (const k of keys) {
  const v = (counts[k] ?? 0) + 1
  seen.push(String(typeof v === 'string' ? v.length > 3 : v))
  seen.push(typeof counts[k])
  seen.push(String(counts[k] === undefined))
  seen.push(String(-counts[k]))
}
console.log(seen.join(' '))

let s = 0
for (const k of Object.keys(counts)) s = s + counts[k]
for (const r of recs) s = s + (r.hits ?? 0.5)
console.log(s)
