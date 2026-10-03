// Loops the group rewrite must leave alone or must not let run ahead:
// an inner remainder whose divisor can be zero (JS gives NaN) next to
// the same loop with a divisor that cannot be, an inner loop reading
// the running total, and a lane that would never finish sitting behind
// an earlier lane that leaves the outer loop.
const seed = Number(process.argv[2] ?? '7')
const n = parseInt(String(seed + 13), 10)

function spin(a: number, b: number): number {
  let k = 0
  while (k < 5) {
    a = (a % b) + 1
    k++
  }
  return a
}

let nanSum = 0
for (let i = 0; i <= n; i++) {
  nanSum += spin(i + 10, i % 3)
}
console.log(nanSum)

let okSum = 0
for (let i = 1; i <= n; i++) {
  okSum += spin(i + 10, (i % 3) + 1)
}
console.log(okSum)

let carried = 1
for (let i = 1; i <= n; i++) {
  let a = i + 100
  let b = carried
  while (b !== 0) {
    const t = b
    b = a % b
    a = t
  }
  carried += a
}
console.log(carried)

// lane i = 1 finishes at once and leaves the loop; lane i = 2 would
// loop forever (x % 7 + 7 is never 0)
let stopped = 0
for (let i = 1; i <= n; i++) {
  let x = i - 1
  while (x !== 0) {
    x = (x % 7) + 7
  }
  stopped += x + i
  if (stopped > 0) {
    break
  }
}
console.log(stopped)
