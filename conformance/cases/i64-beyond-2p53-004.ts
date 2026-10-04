// Integer arithmetic that provably stays inside 2^53: counters that
// start small, small-constant products, sums of such counters.

function triangle(n: number): number {
  let s = 0
  for (let i = 1; i <= n; i++) s += i
  return s
}

let acc = 0
for (let i = 0; i < 1000; i++) {
  for (let j = 0; j < 10; j++) {
    acc = (acc + i * 7 + j * 3 - (i + j)) % 1000003
  }
}
console.log(acc)

let k = 0
let idx = 0
while (k < 100000) {
  idx = k * 31 + 17
  k += 3
}
console.log(k, idx)

const xs: number[] = []
for (let i = 0; i < 10; i++) xs.push(i * 1000 + i)
let total = 0
for (let i = 0; i < xs.length; i++) total += xs[i] + i
console.log(total, triangle(100000), xs[9] - xs[0])

let lo = -5
for (let i = 0; i < 20; i++) lo--
console.log(lo, lo * 2 + 1)
