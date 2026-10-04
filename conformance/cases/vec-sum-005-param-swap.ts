// The array parameter of a sum is replaced by a shorter array after
// its first read: the sum, its length and its data must all come from
// the replacement.
function total(xs: number[], shorter: number[]): number {
  const before = xs.length
  xs = shorter
  let s = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    s = s + xs[i]
  }
  return s + before
}

const n = parseInt(process.argv[2] ?? '1000', 10)
const m = parseInt(process.argv[3] ?? '37', 10)
const long: number[] = []
for (let i = 0; i < n; i = i + 1) {
  long.push(i)
}
const short: number[] = []
for (let i = 0; i < m; i = i + 1) {
  short.push(i * 2)
}
console.log(total(long, short))
