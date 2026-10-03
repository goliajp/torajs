// Outer loop bounds the group rewrite has to read right: an expression
// bound (`n - 1`, `n | 0`), a bound the loop itself rewrites, and a
// step of 2 with `<` at bounds 4k+1 and 4k+2 (a group spans 8).
const seed = Number(process.argv[2] ?? '7')
const div = parseInt(String(720713 + seed), 10)

function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b
    b = a % b
    a = t
  }
  return a
}

function upToMinusOne(n: number, t: number): number {
  let s = 0
  for (let i = 1; i <= n - 1; i++) {
    s += gcd(i, t)
  }
  return s
}

function belowOrZero(n: number, t: number): number {
  let s = 0
  for (let i = 1; i < (n | 0); i++) {
    s += gcd(i, t)
  }
  return s
}

function rewrittenBound(n: number, t: number): number {
  let s = 0
  for (let i = 1; i <= n; i++) {
    s += gcd(i, t)
    if (i === 5) {
      n = 3
    }
  }
  return s
}

function stepTwo(n: number, t: number): number {
  let s = 0
  for (let i = 1; i < n; i += 2) {
    s += gcd(i, t)
  }
  return s
}

for (const k of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 14, 17, 18, 100, 101, 102]) {
  const n = parseInt(String(k), 10)
  console.log(k, upToMinusOne(n, div), belowOrZero(n, div), rewrittenBound(n, div), stepTwo(n, div))
  console.log(k, upToMinusOne(n, 1001), belowOrZero(n, 1001), rewrittenBound(n, 1001), stepTwo(n, 1001))
}
