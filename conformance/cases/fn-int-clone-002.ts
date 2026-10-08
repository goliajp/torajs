// what a recursion over an integer argument can observe stays the
// same when it runs on integers: the sign of a zero, every value it
// prints, an argument past 2^53

function neg(n: number): number {
  if (n <= 0) return n * -1
  return neg(n - 1)
}
console.log(1 / neg(parseInt('3', 10)), 1 / neg(Number('-0')), neg(Number('2.5')) > 0)

function zero(n: number): number {
  if (n <= 0) return n * 0
  return zero(n - 2)
}
console.log(1 / zero(parseInt('4', 10)), 1 / zero(parseInt('5', 10)), 1 / zero(Number('-3')))

function countdown(n: number): number {
  console.log(n)
  if (n <= 0) return 0
  return 1 + countdown(n - 1)
}
console.log(countdown(parseInt('3', 10)), countdown(Number('1.5')), countdown(Number('-0')))

function step(n: number, k: number): number {
  if (k === 0) return n
  return step(n + 1, k - 1)
}
console.log(step(Number('9007199254740990'), parseInt('4', 10)))
console.log(step(Number('4503599627370493'), parseInt('6', 10)))
console.log(step(parseInt('-3', 10), parseInt('5', 10)), step(Number('0.5'), parseInt('3', 10)))
console.log(step(parseInt('12', 10), Number('2.5') - 0.5))

// a negative constant passed along: the window is symmetric
function walk(n: number, d: number): number {
  if (n > 5) return d * n
  return walk(n + 1, -1)
}
console.log(walk(parseInt('-4', 10), parseInt('9', 10)), walk(parseInt('9', 10), Number('2.5')))

// a string beside the number
function rep(s: string, n: number): string {
  if (n <= 0) return ''
  return s + rep(s, n - 1)
}
console.log(rep('ab', parseInt('3', 10)), rep('x', Number('2.5')).length)
