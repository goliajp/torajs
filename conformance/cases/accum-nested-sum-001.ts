// Sums inside an inner loop that the trip count bounds: a counter
// under `d * d <= n` (bounded through the square), a conditional step,
// a sum reset per outer iteration or carried across them, and sums
// whose bound passes 2^53, where the f64 sum must round as JS does.
const seed = Number(process.argv[2] ?? '7')
const k = parseInt(String(seed - 7), 10)

// reset per outer iteration; the counter bounded by its square test
function perRow(n: number): number {
  let total = 0
  for (let i = 1; i <= n; i++) {
    let s = 0
    for (let d = 1; d * d <= i; d++) {
      if (i % 3 === d % 3) s += d * 2
    }
    total += s
  }
  return total
}

// carried across outer iterations: not one episode's sum
function carried(n: number): number {
  let s = 0
  for (let i = 1; i <= n; i++) {
    for (let d = 1; d * d <= i; d++) {
      if ((i + d) % 2 === 0) s += d
    }
  }
  return s
}

// a step skipped on one path through the iteration
function skipping(n: number): number {
  let total = 0
  for (let i = 1; i <= n; i++) {
    let s = 0
    for (let d = 1; d * d <= i; d++) {
      if (d % 4 === 0) continue
      if (i % d === 0) s += d
      else s -= 1
    }
    total += s
  }
  return total
}

// steps large enough that the sum passes 2^53 and rounds
function large(n: number, step: number): number {
  let s = 0
  for (let d = 1; d * d <= n; d++) {
    s += step
  }
  return s
}

// a fractional step: no integer bound applies
function fractional(n: number): number {
  let total = 0
  for (let i = 1; i <= n; i++) {
    let s = 0
    for (let d = 1; d * d <= i; d++) {
      s += d / 4
    }
    total += s
  }
  return total
}

const n = parseInt(String(300 + k), 10)
console.log(perRow(n), carried(n), skipping(n), fractional(n))
console.log(perRow(0), carried(1), skipping(2), fractional(3))
const step = parseInt(String(4503599627370497 + k), 10)
console.log(large(parseInt(String(9 + k), 10), step), large(parseInt(String(100 + k), 10), step))
console.log(large(parseInt(String(1000000 + k), 10), 3), large(-1, 5))
