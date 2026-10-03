// A value every jammed lane computes alike, redefined before the inner
// loop: a lane must read its own definition at each point, not the one
// the first lane left last.
function redefined(limit: number, k: number): number {
  let total = 0
  for (let i = 1; i <= limit; i++) {
    let d = k
    let s = i + d
    d = k + 1
    while (d * d <= i) {
      if (i % d === 0) s = s + d
      d = d + 1
    }
    total = total + s
  }
  return total
}

// the cell set before the inner loop and reset for its test
function resetBeforeTest(limit: number, k: number): number {
  let total = 0
  for (let i = 1; i <= limit; i++) {
    let d = k + 2
    let s = d * 10
    d = 1
    while (d * d <= i) {
      if (i % d === 0) s = s + d
      d = d + 1
    }
    total = total + s
  }
  return total
}

const k = Number(process.argv[2] ?? '1') | 0
console.log(redefined(Number(process.argv[3] ?? '1000') | 0, k))
console.log(redefined(Number(process.argv[4] ?? '37') | 0, k))
console.log(resetBeforeTest(Number(process.argv[5] ?? '500') | 0, k))
console.log(resetBeforeTest(Number(process.argv[6] ?? '9') | 0, k))
