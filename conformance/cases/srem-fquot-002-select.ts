// Divisibility tests a select decides on, which take their quotient
// from a float division when the dividend is a non-negative integer
// below 2^53 and the divisor at least 1, at the edges of that window,
// and the shapes that keep the integer remainder: a divisor that can
// be zero, a dividend that can be negative or fractional, and a select
// feeding the divisor back.
const seed = Number(process.argv[2] ?? '7')
const k = parseInt(String(seed - 7), 10)

// count and sum of the divisors of every x in [lo, hi] up to `top`
function tally(lo: number, hi: number, top: number): number {
  let c = 0
  let s = 0
  for (let x = lo; x <= hi; x++) {
    for (let d = 1; d <= top; d++) {
      c = x % d === 0 ? c + 1 : c
      s = x % d !== 0 ? s : s + d
    }
  }
  return c * 1000003 + s
}

// integer dividends and divisors only, the dividend never negative:
// the test takes the float quotient
function counted(n: number, top: number): number {
  let c = 0
  for (let x = 0; x <= n; x++) {
    for (let d = 1; d <= top; d++) {
      c = x % d === 0 ? c + d : c
    }
  }
  return c
}

// one dividend, which may sit past 2^53 (a loop over such dividends
// would not advance)
function divisorsOf(x: number, top: number): number {
  let c = 0
  for (let d = 1; d <= top; d++) {
    c = x % d === 0 ? c + d : c
  }
  return c
}

// the divisor starts at `from`, which may be zero (x % 0 is NaN)
function fromDivisor(n: number, from: number): number {
  let c = 0
  for (let x = 0; x < n; x++) {
    for (let d = from; d < from + 5; d++) {
      c = x % d === 0 ? c + d : c - 1
    }
  }
  return c
}

// the select's result is the next divisor
function stepped(n: number): number {
  let t = 0
  for (let x = 1; x <= n; x++) {
    let d = 1
    while (d * d <= x) {
      d = x % d === 0 ? d + 1 : d + 2
      t = t + d
    }
  }
  return t
}

const lim = parseInt(String(60 + k), 10)
console.log(tally(0, lim, 12), tally(1, 1, 1), tally(0, 0, 3))
console.log(tally(-20 + k, 20, 7), tally(parseFloat(String(0.5 + k)), 9.5, 4))
const top = parseInt(String(9007199254740991 + k), 10)
console.log(tally(top - 3, top, 9), divisorsOf(top, 30), divisorsOf(top + 1, 30))
console.log(divisorsOf(top + 3, 30), divisorsOf(top * 4, 30), divisorsOf(-top, 30))
console.log(counted(lim, 12), counted(0, 5), counted(parseInt(String(1000 + k), 10), 37))
console.log(fromDivisor(lim, 0), fromDivisor(lim, 1), fromDivisor(lim, -3))
console.log(stepped(lim), stepped(parseInt(String(1 + k), 10)))

// constant ranges: the facts bound both operands, so these take the
// float quotient, up to the last dividend below 2^53
let small = 0
for (let x = 0; x <= 300; x++) {
  for (let d = 1; d <= 12; d++) {
    small = x % d === 0 ? small + d : small
  }
}
let edge = 0
for (let x = 9007199254740983; x <= 9007199254740991; x++) {
  for (let d = 1; d <= 40; d++) {
    edge = x % d !== 0 ? edge : edge + d
  }
}
console.log(small, edge)
