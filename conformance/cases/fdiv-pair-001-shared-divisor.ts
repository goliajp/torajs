// The jammed lanes of a trial-division loop divide their own dividend
// by the shared divisor; two lanes' quotients come from one two-lane
// divide. Ranges of every length around the group size, ranges that
// start at zero or below it, dividends around 2^31 (where the integer
// copy of the loop ends) and past it, perfect squares and primes, and
// a divisor that starts above one.
const seed = Number(process.argv[2] ?? '7')
const k = parseInt(String(seed - 7), 10)

function sumTo(hi: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s = 0
    let d = 1
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total + s
  }
  return total
}

// the dividends start at zero, past a thousand, and at a bound that
// may be negative or fractional (the loop then keeps the remainder)
function sumFromZero(hi: number): number {
  let total = 0
  for (let n = 0; n <= hi; n++) {
    let s = 0
    let d = 1
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total * 7 + s
    total = total % 1000000007
  }
  return total
}

function sumFromThousand(hi: number): number {
  let total = 0
  for (let n = 1000; n <= hi; n++) {
    let s = 0
    let d = 1
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total + s
  }
  return total
}

function sumRange(lo: number, hi: number): number {
  let total = 0
  for (let n = lo; n <= hi; n++) {
    let s = 0
    let d = 1
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total + s
  }
  return total
}

// counts the divisors from `from` up to the square root, weighted by
// position so a lane mix-up changes the result
function countFrom(hi: number, from: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let c = 0
    let d = from
    while (d * d <= n) {
      if (n % d === 0) c = c + 1
      d = d + 1
    }
    total = total * 3 + c
    total = total % 1000000007
  }
  return total
}

for (let len = 0; len <= 10; len++) {
  const hi = parseInt(String(len + k), 10)
  console.log(len, sumTo(hi), sumFromZero(hi), sumFromThousand(hi + 1000))
}
console.log(sumTo(parseInt(String(-3 + k), 10)), sumTo(parseFloat(String(9.5 + k))))
console.log(sumFromZero(parseInt(String(5000 + k), 10)), sumFromThousand(4000 + k))
console.log(sumTo(parseInt(String(100000 + k), 10)), sumTo(NaN))
console.log(sumRange(-12 + k, 12), sumRange(parseFloat(String(0.5 + k)), 20))
const sq = parseInt(String(1000000 + k), 10)
console.log(sumRange(sq - 3, sq + 3), sumRange(sq * sq - 2, sq * sq + 1))
const edge = parseInt(String(2147483647 + k), 10)
console.log(sumRange(edge - 5, edge), sumRange(edge - 1, edge + 4))
const prime = parseInt(String(1000003 + k), 10)
console.log(sumRange(prime - 1, prime + 1), sumRange(prime * 7, prime * 7 + 2))
for (const from of [1, 2, 3, 7]) {
  const f = parseInt(String(from + k), 10)
  console.log(from, countFrom(parseInt(String(3000 + k), 10), f), countFrom(9, f))
}
