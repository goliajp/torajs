type Rec = { key: string; hits?: number; n: number }

const n = parseInt(process.argv[2], 10)
const recs: Rec[] = []
for (let i = 0; i < 64; i = i + 1) {
  if (i % 3 === 0) {
    recs.push({ key: "k" + i, n: i })
  } else {
    recs.push({ key: "k" + i, hits: i, n: i })
  }
}
const counts: Record<string, number> = {}
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const r = recs[i & 63]
  counts[r.key] = (counts[r.key] ?? 0) + 1
  total = total + (r.hits ?? 1) + r.n
}
let sum = 0
for (const k of Object.keys(counts)) {
  sum = sum + counts[k]
}
console.log(total, sum, Object.keys(counts).length)
