// `d[r.key]` and `d[r.key] = (d[r.key] ?? 0) + 1` on a
// `Record<string, number>` with the key a string field of a record:
// the key the read and the write use is the value the field held when
// each key expression ran, even when code that runs inside the access
// or on the right-hand side rewrites the field, drops the record
// itself, or allocates strings of the same size in the meantime. That
// code runs from an inherited getter, a proxy trap that hands the key
// back, and the right-hand side of the write.

type Rec = { key: string; n: number }
const recs: Rec[] = []
for (let i = 0; i < 6; i = i + 1) recs.push({ key: 'key' + i + 'x', n: i })

const d: Record<string, number> = {}
let churn: string[] = []
function rewrite(r: Rec, tag: string): void {
  r.key = 'new' + tag + r.n
  churn = []
  for (let i = 0; i < 64; i = i + 1) churn.push('zzz' + i + 'y')
}

// an inherited getter for one key rewrites the field it was read from
Object.defineProperty(Object.prototype, 'key2x', {
  get() {
    rewrite(recs[2], 'g')
    return 40
  },
  set(v) {
    Object.defineProperty(this, 'key2x', { value: v, writable: true, enumerable: true, configurable: true })
  },
  configurable: true,
})
const r2 = recs[2]
console.log(d[r2.key], r2.key, churn.length)
r2.key = 'key2x'
d[r2.key] = (d[r2.key] ?? 0) + 1
console.log(JSON.stringify(d), r2.key)

// the right-hand side rewrites the field the write's key came from
const r3 = recs[3]
d[r3.key] = (() => {
  rewrite(r3, 'w')
  return 7
})()
console.log(JSON.stringify(d), r3.key)

// the right-hand side drops the only other holder of the record
let held: Rec | null = { key: 'held' + recs.length + 'q', n: 9 }
const keep = held
d[keep.key] = (() => {
  held = null
  keep.key = 'gone' + keep.n
  churn = []
  for (let i = 0; i < 64; i = i + 1) churn.push('vvvvv' + i + 'w')
  return 3
})()
console.log(JSON.stringify(d), keep.key, held)

// a proxy's get trap rewrites the field and answers with the key it got
const p: Record<string, number> = new Proxy({} as Record<string, number>, {
  get(_t, k) {
    rewrite(recs[4], 'p')
    return String(k).length
  },
})
const r4 = recs[4]
console.log(p[r4.key], r4.key)
console.log(p[r4.key] + p[recs[5].key])

// the loop shape the bench runs, with the field rewritten every 97 rounds
const counts: Record<string, number> = {}
for (let i = 0; i < 2000; i = i + 1) {
  const r = recs[i % 6]
  counts[r.key] = (counts[r.key] ?? 0) + 1
  if (i % 97 === 0) r.key = 'k' + (i % 5)
}
let sum = 0
for (const k of Object.keys(counts)) sum = sum + counts[k]
console.log(sum, Object.keys(counts).length, Object.keys(counts).sort().join(','))
