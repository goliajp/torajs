// Control flow after the inner loop: `continue`, a labeled break out of
// an enclosing loop, and a throw caught outside the loop, each taken by
// whichever lane of a group reaches it first in iteration order.
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

function oddOnly(n: number, t: number): number {
  let s = 0
  for (let i = 1; i <= n; i++) {
    const g = gcd(i, t)
    if (g % 2 === 0) {
      continue
    }
    s += g
  }
  return s
}

function breakOuter(n: number, t: number, limit: number): number {
  let s = 0
  outer: for (let r = 0; r < 3; r++) {
    for (let i = 1; i <= n; i++) {
      s += gcd(i, t)
      if (s > limit) {
        break outer
      }
    }
    s += 1000
  }
  return s
}

function throwAt(n: number, t: number, stop: number): string {
  let s = 0
  try {
    for (let i = 1; i <= n; i++) {
      s += gcd(i, t)
      if (i === stop) {
        throw new Error('stop at ' + i)
      }
    }
  } catch (e) {
    return (e as Error).message + ' ' + s
  }
  return 'done ' + s
}

for (const k of [0, 1, 4, 5, 9, 40]) {
  const n = parseInt(String(k), 10)
  console.log(k, oddOnly(n, div), oddOnly(n, 1001))
  for (const limit of [0, 3, 10, 50, 1000000]) {
    console.log(k, limit, breakOuter(n, div, limit), breakOuter(n, 1001, limit))
  }
  for (const stop of [1, 2, 3, 4, 5, 8]) {
    console.log(k, stop, throwAt(n, div, stop))
  }
}
