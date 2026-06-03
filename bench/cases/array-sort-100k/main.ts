// Sort a 1000-element pseudo-random array 100 times. The RNG step
// stays inside int32 to keep f64-vs-i64 multiplication semantics
// out of the bench surface (tora i64 multiply diverges from JS f64
// when the intermediate product crosses 2^53 — bench would then
// be measuring two different programs).
function build(n: number, seed: number): number[] {
  const xs: number[] = []
  let s = seed | 0
  for (let i = 0; i < n; i = i + 1) {
    s = ((s * 48271) | 0) & 0x7fffffff
    if (s === 0) s = 1
    xs.push(s)
  }
  return xs
}

let checksum = 0
const passes = 100
for (let p = 0; p < passes; p = p + 1) {
  const xs = build(1000, p + 1)
  xs.sort((a, b) => a - b)
  checksum = checksum + xs[0] + xs[999]
}
console.log(checksum)
