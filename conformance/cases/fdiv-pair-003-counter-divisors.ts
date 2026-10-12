// Divisors that are loop counters, read by paired divides from vector
// cells stepped with the counter: one counter over the jammed lanes of
// a divisor-sum loop; four counters with different steps in one loop,
// each dividing two dividends, or dividing its own; a counter stepped
// before the tests read it, one stepped only on some trips, and one a
// `continue` skips the step of.
const seed = Number(process.argv[2] ?? '7')
const k = parseInt(String(seed - 7), 10)

function num(x: number): number {
  return parseFloat(String(x + k))
}

function sumTo(hi: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s = 0
    for (let d = 1; d * d <= n; d++) {
      if (n % d === 0) s += d
    }
    total = (total * 7 + s) % 1000000007
  }
  return total
}

function fourShared(x: number, y: number, n: number): number {
  const a = (x & 0xfffff) + 1
  const b = (y & 0xfffff) + 1
  let s1 = 0, s2 = 0, s3 = 0, s4 = 0, s5 = 0, s6 = 0, s7 = 0, s8 = 0
  for (let i = 1, j = 1, k = 1, l = 1; i < n; i++, j += 2, k += 3, l += 5) {
    if (a % i === 0) s1 += i
    if (b % i === 0) s2 += i
    if (a % j === 0) s3 += j
    if (b % j === 0) s4 += j
    if (a % k === 0) s5 += k
    if (b % k === 0) s6 += k
    if (a % l === 0) s7 += l
    if (b % l === 0) s8 += l
  }
  return s1 + 3 * s2 + 5 * s3 + 7 * s4 + 11 * s5 + 13 * s6 + 17 * s7 + 19 * s8
}

function fourOwn(hi: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s1 = 0, s2 = 0, s3 = 0, s4 = 0
    for (let i = 1, j = 2, k = 3, l = 4; i * i <= n; i++, j += 2, k += 3, l += 5) {
      if (n % i === 0) s1 += i
      if (n % j === 0) s2 += j
      if (n % k === 0) s3 += k
      if (n % l === 0) s4 += l
    }
    total = (total * 7 + s1 + 3 * s2 + 5 * s3 + 11 * s4) % 1000000007
  }
  return total
}

// the counter steps before the test reads it
function stepFirst(hi: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s = 0
    let d = 0
    while (d * d < n) {
      d = d + 1
      if (n % d === 0) s = s + d
    }
    total = (total * 7 + s) % 1000000007
  }
  return total
}

// the counter steps only when the dividend is odd at it
function stepSome(hi: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s = 0
    let d = 1
    for (let t = 0; t < 40 && d <= n; t++) {
      if (n % d === 0) s = s + d
      if ((n + t) % 3 !== 0) d = d + 1
    }
    total = (total * 7 + s) % 1000000007
  }
  return total
}

// a `continue` skips the step on some trips
function stepSkipped(hi: number): number {
  let total = 0
  for (let n = 1; n <= hi; n++) {
    let s = 0
    let d = 1
    for (let t = 0; t < 40 && d <= n; t++) {
      if (n % d === 0) s = s + d
      if (t % 4 === 1) continue
      d = d + 1
    }
    total = (total * 7 + s) % 1000000007
  }
  return total
}

for (let len = 0; len <= 9; len++) {
  console.log(len, sumTo(num(len)), fourOwn(num(len)), stepFirst(num(len)))
  console.log(stepSome(num(len)), stepSkipped(num(len)))
}
console.log(sumTo(num(3000)), fourOwn(num(3000)), stepFirst(num(3000)))
console.log(stepSome(num(3000)), stepSkipped(num(3000)))
let t = 0
for (let r = 0; r < 20; r++) {
  t = (t * 31 + fourShared(num(720720 + r), num(5040 + r), num(3000))) % 1000000007
}
console.log(t, fourShared(num(0), num(-1), num(50)))
