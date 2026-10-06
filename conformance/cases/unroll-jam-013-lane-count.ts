// Lanes that step in lockstep (the inner test reads only values every
// lane shares) run a wider group: seven for an integer remainder
// chain, eight for a float divide chain. Every group boundary is
// crossed: outer trip counts from 0 to 20, negative and fractional
// steps of the recurrence, a dividend equal to the divisor, and outer
// counters starting near 2^53 so the group bound sits by the edge.
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

const shift = parseInt(String(seed - 7), 10)
for (let n = 0; n <= 20; n++) {
  const hi = n + shift
  console.log(
    n,
    sumSurvivors(1, hi, 10),
    sumSurvivors(-hi, -1, 10),
    sumSurvivors(-3, hi - 4, 6),
    sumSurvivors(2, hi + 1, 2),
    sumSurvivors(2 ** 53 - hi - 1, 2 ** 53 - 1, 5),
  )
  console.log(n, sumWalks(1, hi, 3, 5), sumWalks(-hi, hi, 2, 4), sumWalks(0, hi, 1, 1))
}
// a non-integral step of the recurrence keeps every lane on the float path
let frac = 0
for (let k = 0; k <= 15 + shift; k++) {
  frac += survivor(9, k + 0.5)
}
console.log(frac)
