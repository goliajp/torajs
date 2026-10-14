// A loop counter whose bound the range analysis reaches only after
// its round budget runs out: the main body carries enough other loops
// and compared constants that `k` is still climbing when the ascent
// stops widening through thresholds. `k < 30` must stay a real test.
const seed = Number(process.argv[2] ?? '7')
const pool = parseInt(String(seed + 3), 10)
const neg = parseInt(String(-seed + 4), 10)
function show(r: number): string {
  return String(r)
}
function rems(from: number, to: number, p: number): string {
  const out: string[] = []
  for (let i = from; i < to; i = i + 1) out.push(show(i % p))
  return out.join(',')
}
console.log(rems(0, 12, neg), rems(-6, 6, neg))
console.log(rems(0, 8, pool / 4), rems(0, 8, 2.5))
console.log(rems(0, 5, 1), rems(0, 5, 2 ** 53), rems(0, 3, 2 ** 60))
let p = pool
for (let i = 0; i < 40; i = i + 1) {
  if (i === 17) p = p + 2
}
let k = 0
let trips = 0
while (k < 30 && trips < 100) {
  if (trips % 3 !== 0) k = k + 1
  trips = trips + 1
}
console.log(k, trips, p)
