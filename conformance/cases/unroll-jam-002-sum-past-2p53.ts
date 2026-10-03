// The running total of a jammed loop crosses 2^53 part way through a
// group — at each lane position in turn, chosen by the starting total:
// from there on the sum is a float sum, added in the original order,
// and the later lanes' inner loops, already run, must not leak into it.
const seed = Number(process.argv[2] ?? '7')
const base = parseInt(String(2 ** 51 - 7 + seed), 10)
const div = parseInt(String(1234560 + seed), 10)

function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b
    b = a % b
    a = t
  }
  return a
}

function sumPlus(init: number, lo: number, hi: number, t: number): number {
  let total = init
  for (let i = lo; i <= hi; i++) {
    total += gcd(i, t) + i
  }
  return total
}

const starts = [0, 2 ** 51, 2 ** 52, 2 ** 52 + 2 ** 51, 2 ** 53 - 3, 2 ** 53, -(2 ** 53)]
for (const s of starts) {
  const init = parseInt(String(s), 10)
  for (let k = 0; k <= 9; k++) {
    console.log(k, sumPlus(init, base, base + k, div), sumPlus(init, base - k, base + 2, 1))
  }
}
