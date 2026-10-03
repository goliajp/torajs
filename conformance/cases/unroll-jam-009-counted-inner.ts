// A counted inner loop (every lane runs the same number of steps) whose
// divisor is its own counter, read through a per-step copy:
// the Josephus survivor r = (r + k) % i, for a range of k.
const seed = Number(process.argv[2] ?? '7')

function survivor(people: number, k: number): number {
  let r = 0
  let i = 2
  while (i <= people) {
    r = (r + k) % i
    i++
  }
  return r
}

function sumSurvivors(lo: number, hi: number, people: number): number {
  let total = 0
  for (let k = lo; k <= hi; k++) {
    total += survivor(people, k)
  }
  return total
}

for (const people of [0, 1, 2, 3, 7, 41]) {
  const p = parseInt(String(people), 10)
  for (const hi of [0, 1, 3, 4, 5, 9, 100]) {
    const h = parseInt(String(hi + seed - 7), 10)
    console.log(people, hi, sumSurvivors(1, h, p), sumSurvivors(2 ** 52 - 3, 2 ** 52 + h, p))
  }
}
