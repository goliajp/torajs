// Sums of divisors up to the square root, summed over a range: the
// inner `if (n % d === 0) s += d` becomes a select, the outer loop is
// jammed with every lane stepping one shared divisor, and the test
// takes its quotient from a float division. Ranges whose length is and
// is not a multiple of the group size, empty ones, negative and
// non-integer bounds, bounds around 2^31 (where the integer copy of
// the loop ends), a start past zero, and a sum that leaves the loop
// early.
const seed = Number(process.argv[2] ?? '7')
const k = parseInt(String(seed - 7), 10)

function divisorSum(n: number): number {
  let s = 0
  let d = 1
  while (d * d <= n) {
    if (n % d === 0) s = s + d
    d = d + 1
  }
  return s
}

function sumTo(limit: number): number {
  let total = 0
  let n = 1
  while (n <= limit) {
    total = total + divisorSum(n)
    n = n + 1
  }
  return total
}

function sumFrom(lo: number, hi: number): number {
  let total = 0
  for (let n = lo; n <= hi; n++) {
    total += divisorSum(n)
  }
  return total
}

// leaves the loop once the total passes `cap`: a lane past the one
// that leaves must not have added anything
function sumUntil(limit: number, cap: number): number {
  let total = 0
  let n = 1
  while (n <= limit) {
    total = total + divisorSum(n)
    if (total > cap) return total * 1000 + n
    n = n + 1
  }
  return -total
}

for (const b of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 100, 1001]) {
  const limit = parseInt(String(b + k), 10)
  console.log(b, sumTo(limit), sumFrom(limit - 9, limit), sumFrom(-limit, 3))
}
console.log(sumTo(parseFloat(String(10.5 + k))), sumTo(parseFloat(String(-2.5 + k))))
console.log(sumTo(NaN), sumTo(-Infinity), sumFrom(parseFloat(String(0.5 + k)), 20))
const edge = parseInt(String(2147483647 + k), 10)
console.log(sumFrom(edge - 6, edge), sumFrom(edge - 2, edge + 3))
const big = parseInt(String(1000000007 + k), 10)
console.log(sumFrom(big, big + 4), sumFrom(big * 1000, big * 1000 + 1))
for (const cap of [0, 1, 5, 30, 31, 32, 1000]) {
  console.log(cap, sumUntil(parseInt(String(200 + k), 10), cap))
}
