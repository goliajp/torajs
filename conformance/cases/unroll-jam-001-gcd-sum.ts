// A counted loop summing an inner Euclid loop per iteration: groups of
// iterations step their inner loops together, the rest run one by one.
// Bounds that are and are not a multiple of the group size, empty and
// single-iteration loops, and neighbours whose inner trip counts differ
// a lot (consecutive Fibonacci numbers next to their neighbours).
const seed = Number(process.argv[2] ?? '7')
const big = parseInt(String(1234560 + seed), 10)
const many = parseInt(String(720713 + seed), 10)
const fib45 = parseInt(String(1134903163 + seed), 10)
const fib44 = parseInt(String(701408726 + seed), 10)

function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b
    b = a % b
    a = t
  }
  return a
}

function sumGcd(lo: number, hi: number, t: number): number {
  let total = 0
  for (let i = lo; i <= hi; i++) {
    total += gcd(i, t)
  }
  return total
}

function sumGcdBelow(n: number, t: number): number {
  let total = 0
  for (let i = 0; i < n; i += 3) {
    total += gcd(t, i)
  }
  return total
}

const ns = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 13, 100, 1001]
for (const n of ns) {
  const hi = parseInt(String(n), 10)
  console.log(n, sumGcd(1, hi, big), sumGcd(1, hi, many), sumGcd(1, hi, 1), sumGcd(1, hi, 0))
  console.log(n, sumGcdBelow(hi, many), sumGcd(-hi, hi, many))
}
for (let d = -6; d <= 6; d++) {
  console.log(d, sumGcd(fib44 - 5, fib44 + d, fib45), sumGcd(fib44 + d, fib44 + 9, fib45))
}
console.log(gcd(fib44, fib45), gcd(fib44 + 1, fib45))
