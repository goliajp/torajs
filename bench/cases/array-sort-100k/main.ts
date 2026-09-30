// Sort a 1000-element pseudo-random array 100 times. The RNG is the
// minimal standard generator: its product stays below 2^47, so f64
// and i64 multiplication agree and every runtime sorts the same
// numbers.
function build(n: number, seed: number): number[] {
  const xs: number[] = []
  let s = seed | 0
  for (let i = 0; i < n; i = i + 1) {
    s = (s * 48271) % 2147483647
    if (s === 0) s = 1
    xs.push(s)
  }
  return xs
}

let checksum = 0
const passes = parseInt(process.argv[2], 10)
const len = parseInt(process.argv[3], 10)
for (let p = 0; p < passes; p = p + 1) {
  const xs = build(len, p + 1)
  xs.sort((a, b) => a - b)
  checksum = checksum + xs[0] + xs[len - 1]
}
console.log(checksum)
