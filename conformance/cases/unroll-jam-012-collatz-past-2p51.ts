// Collatz step counts under a counted outer loop: the inner step is a
// multiply-add or a halving, no divide. Near 2^50 an odd start jumps
// past 2^51 on its first step, where the integer form hands over to
// the float one; whether that happens differs from one start to the
// next inside a group of consecutive starts, and some cross it only
// after a few steps.
const seed = Number(process.argv[2] ?? '0')

function steps(n: number): number {
  let count: number = 0
  while (n !== 1) {
    if (n % 2 === 0) {
      n = n / 2
    } else {
      n = 3 * n + 1
    }
    count = count + 1
  }
  return count
}

function longest(lo: number, hi: number): number {
  let max: number = 0
  let i: number = lo
  while (i <= hi) {
    const s: number = steps(i)
    if (s > max) {
      max = s
    }
    i = i + 1
  }
  return max
}

// the first start with the most steps: lanes must finish in order
function firstLongest(lo: number, hi: number): number {
  let max: number = -1
  let at: number = 0
  for (let i = lo; i <= hi; i++) {
    const s: number = steps(i)
    if (s > max) {
      max = s
      at = i - lo
    }
  }
  return max * 100000 + at
}

console.log(longest(1, 3000 + seed))
console.log(firstLongest(1, 2001 + seed))
const p50 = 1125899906842624
console.log(longest(p50 - 37 + seed, p50 + 41 + seed))
console.log(firstLongest(p50 - 22 + seed, p50 + 19 + seed))
// past 2^51 only after a few steps, some lanes alone by then
console.log(firstLongest(p50 / 2 - 30 + seed, p50 / 2 + 30 + seed))
console.log(longest(p50 / 4 - 30 + seed, p50 / 4 + 30 + seed))
console.log(longest(3 * p50 - 9 + seed, 3 * p50 + 9 + seed))
