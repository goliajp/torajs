// A value only the inner loop sets (`t`, the divisor of the last step)
// read after the outer loop: it must be the last iteration's last
// step, even when that iteration finished its inner loop in step with
// the others of its group.
const seed = Number(process.argv[2] ?? '7')
const div = parseInt(String(seed), 10)

let s = 0
let t = 0
for (let i = 4; i <= 7; i++) {
  let a = i
  let b = div
  while (b !== 0) {
    t = b
    const r = a % b
    a = b
    b = r
  }
  s += a
}
console.log(s, t)

function lastDivisor(lo: number, hi: number, d: number): number {
  let total = 0
  let last = -1
  for (let i = lo; i <= hi; i++) {
    let a = i
    let b = d
    while (b !== 0) {
      last = b
      const r = a % b
      a = b
      b = r
    }
    total += a
  }
  return total * 1000 + last
}

function lastDivisorEarlyOut(lo: number, hi: number, d: number, cap: number): number {
  let total = 0
  let last = -1
  for (let i = lo; i <= hi; i++) {
    let a = i
    let b = d
    while (b !== 0) {
      last = b
      const r = a % b
      a = b
      b = r
    }
    total += a
    if (total > cap) {
      return total * 1000 + last
    }
  }
  return -(total * 1000 + last)
}

for (const hi of [0, 3, 4, 5, 7, 8, 11, 12, 13, 40]) {
  const h = parseInt(String(hi), 10)
  console.log(
    hi,
    lastDivisor(1, h, div),
    lastDivisor(4, h, 1234567),
    lastDivisorEarlyOut(1, h, 1234567, 5),
    lastDivisorEarlyOut(1, h, 720720, 20),
  )
}
