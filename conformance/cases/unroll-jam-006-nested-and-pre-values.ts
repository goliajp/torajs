// Two transformable loops in one function, inside an enclosing loop,
// and an inner loop that reads a value the iteration computed before
// it (`m = i * 3 + 1`), not only the counter — once inside the
// enclosing loop, once as a loop of its own.
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

function twoLoops(n: number, t: number): number {
  let total = 0
  for (let r = 1; r <= 3; r++) {
    let s1 = 0
    for (let i = 1; i <= n; i++) {
      s1 += gcd(i, t + r)
    }
    let s2 = 0
    for (let i = 1; i <= n; i++) {
      const m = i * 3 + 1
      let a = i
      let b = t
      while (b !== 0) {
        const x = b
        b = (a + m) % b
        a = x
      }
      s2 += a
    }
    total += s1 * 7 + s2
  }
  return total
}

function preValue(n: number, t: number): number {
  let s = 0
  for (let i = 1; i <= n; i++) {
    const m = i * 3 + 1
    let a = i
    let b = t
    while (b !== 0) {
      const x = b
      b = (a + m) % b
      a = x
    }
    s += a
  }
  return s
}

for (const k of [0, 1, 3, 4, 5, 11, 64, 333]) {
  const n = parseInt(String(k), 10)
  console.log(k, twoLoops(n, div), twoLoops(n, 97), preValue(n, div), preValue(n, 97))
}
