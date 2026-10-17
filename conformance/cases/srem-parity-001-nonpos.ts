// `x % 2^k` tested only against zero, over a dividend that never
// goes above zero: the remainder is in [-(2^k - 1), 0], and the mask
// that stands in for it reads the non-zero cases as positive, so a
// compare decided from both ranges at once would see only zero. Odd
// and even counts over constant and argv bounds, `!==` and `===`,
// divisors 2, 4 and 8, and a count that is read after the loop.
const n = Number(process.argv[2] ?? '10')

let odd = 0
for (let i = 0; i > -10; i--) {
  if (i % 2 !== 0) odd++
}
console.log(odd)

let odd2 = 0
let four = 0
let eight = 0
for (let i = 0; i > -n; i--) {
  if (i % 2 !== 0) odd2++
  if (i % 4 === 0) four++
  if (i % 8 !== 0) eight++
}
console.log(odd2, four, eight)

function evens(from: number): number {
  let c = 0
  for (let i = from; i <= 0; i++) {
    if (i % 2 === 0) c++
  }
  return c
}
console.log(evens(-7), evens(-n), evens(0))
