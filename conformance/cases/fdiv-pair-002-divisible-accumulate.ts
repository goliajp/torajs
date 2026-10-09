// The jammed lanes of a divisor-sum loop test divisibility and add the
// divisor to their own sums on the two-lane divide's lanes, and split
// the sums back out when the shared loop ends. Ranges of every length
// around the group size, dividends from zero, around 2^31 (where the
// integer copy of the loop ends), around a perfect square and a prime,
// and near 10^12; divisors that start above one and sums that start
// away from zero, the same in every lane or not; and a divisor start
// read at run time, which keeps the scalar test. Each dividend range
// starts at a constant, so the dividends are known not to be negative.
const seed = Number(process.argv[2] ?? '7')
const k = parseInt(String(seed - 7), 10)

function num(x: number): number {
  return parseFloat(String(x + k))
}

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

// weighted by position so a lane mix-up changes the result
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

function sumNearEdge(hi: number): number {
  let total = 0
  for (let n = 2147483640; n <= hi; n++) {
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

// 1000^2 and the prime 1000003
function sumNearSquare(hi: number): number {
  let total = 0
  for (let n = 999995; n <= hi; n++) {
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

// around 10^12 = (10^6)^2
function sumNearTera(hi: number): number {
  let total = 0
  for (let n = 999999999998; n <= hi; n++) {
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

// divisors from 2, the sum starting at 1
function sumFrom2(hi: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s = 1
    let d = 2
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total * 7 + s
    total = total % 1000000007
  }
  return total
}

// divisors from 3, the sum starting at the dividend (a float sum,
// which keeps the scalar test)
function sumFrom3(hi: number): number {
  let total = 0
  for (let n = 0; n <= hi; n++) {
    let s = n
    let d = 3
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total * 7 + s
    total = total % 1000000007
  }
  return total
}

// divisors from 7, the sum starting at the dividend's low bits
function sumFrom7(hi: number): number {
  let total = 0
  for (let n = 40; n <= hi; n++) {
    let s = n & 7
    let d = 7
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total * 7 + s
    total = total % 1000000007
  }
  return total
}

function sumFrom(hi: number, from: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s = 0
    let d = from
    while (d * d <= n) {
      if (n % d === 0) s = s + d
      d = d + 1
    }
    total = total * 7 + s
    total = total % 1000000007
  }
  return total
}

for (let len = 0; len <= 10; len++) {
  console.log(len, sumTo(num(len)), sumFromZero(num(len - 1)), sumFrom2(num(len)))
  console.log(sumFrom3(num(len - 1)), sumFrom7(num(39 + len)), sumNearEdge(num(2147483639 + len)))
  console.log(sumNearSquare(num(999994 + len)), sumNearTera(num(999999999997 + len)))
}
console.log(sumTo(num(-3)), sumTo(num(9.5)), sumTo(NaN), sumFromZero(num(-1)))
console.log(sumTo(num(100000)), sumFromZero(num(5000)), sumFrom2(num(3000)), sumFrom3(num(3001)))
console.log(sumNearEdge(num(2147483660)), sumNearSquare(num(1000010)), sumFrom7(num(2000)))
for (const from of [1, 2, 3, 7, 0, -2]) {
  console.log(from, sumFrom(num(3000), num(from)), sumFrom(num(9), num(from)))
}
