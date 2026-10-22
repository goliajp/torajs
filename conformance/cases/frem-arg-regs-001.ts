// `%` lowers to a call to fmod with its operands in d0 / d1. A
// parameter whose last read is that call may still sit in d0, as the
// right operand, or as the left with the right in d1. The functions are
// called through an array so each keeps its own parameter registers.
function rhsInD0(a: number, b: number): number {
  return b % a
}
function swapped(a: number, b: number): number {
  return b % a + a % b
}
function lhsElsewhere(a: number): number {
  const y = a * 2 + 1
  return y % a
}
const two: ((a: number, b: number) => number)[] = [rhsInD0, swapped]
const one: ((a: number) => number)[] = [lhsElsewhere]
const n = process.argv.length + 1.5
let s = 0
for (let i = 0; i < 4; i = i + 1) {
  s = s + two[i % 2](n + i, 17.25) + one[0](n + i)
}
console.log(two[0](n, 17.25), two[1](n, 7.5), one[0](n), s)
