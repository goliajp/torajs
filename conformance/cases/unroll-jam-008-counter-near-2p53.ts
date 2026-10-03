// The outer counter runs up to 2^53 - 1: lanes i + 1 .. i + 3 of the
// last groups sit at the top of the exact integer range.
const seed = Number(process.argv[2] ?? '7')
const div = parseInt(String(720713 + seed), 10)
const top = parseInt(String(2 ** 53 - 1), 10)

function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b
    b = a % b
    a = t
  }
  return a
}

function sumRange(lo: number, hi: number, t: number): number {
  let s = 0
  for (let i = lo; i <= hi; i++) {
    s += gcd(i, t)
  }
  return s
}

for (let k = 0; k <= 9; k++) {
  console.log(k, sumRange(top - k, top, div), sumRange(top - 20, top - k, div), sumRange(top - k, top, 3))
}
