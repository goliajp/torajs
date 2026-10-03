// A float division on the inner recurrence, `y = x / (y - k)`: step 1
// divides by zero (Infinity), x = 0 gives 0 / 0 (NaN), and the sums
// must carry both like bun's. A continued-fraction tail through
// Math.floor (a runtime call, so left alone) rides along as a control.
const seed = Number(process.argv[2] ?? '7')
const d = parseInt(String(seed), 10)

function walk(x: number, steps: number): number {
  let y = x
  let k = 0
  while (k < steps) {
    y = x / (y - k)
    k++
  }
  return y
}

function sumWalks(lo: number, hi: number, den: number, steps: number): number {
  let s = 0
  for (let i = lo; i <= hi; i++) {
    s += walk(i / den, steps)
  }
  return s
}

function tail(x: number, steps: number): number {
  let y = x
  let k = 0
  while (k < steps) {
    const f = Math.floor(y)
    y = 1 / (y - f)
    k++
  }
  return y
}

function sumTails(n: number, den: number, steps: number): number {
  let s = 0
  for (let i = 1; i <= n; i++) {
    s += tail(i / den, steps)
  }
  return s
}

for (const k of [0, 1, 5, 6, 7, 8, 13]) {
  const n = parseInt(String(k), 10)
  console.log(k, sumWalks(1, n, d, 1), sumWalks(1, n, d, 2), sumWalks(1, n, d, 3), sumWalks(1, n, d, 7))
  console.log(k, sumWalks(0, n, d, 3), sumWalks(-n, n, d, 5), sumTails(n, d, 4), sumTails(n, Math.SQRT2, 5))
}
