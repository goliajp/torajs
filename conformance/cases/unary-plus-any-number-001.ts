// `+x` over an `any` operand is ToNumber (§13.5.4): a Number for every
// input, or the TypeError that Symbol and BigInt operands raise.
const xs: any[] = [3, 2.5, -0, '12', ' 7 ', 'x', '', true, false, null, undefined,
  [5], [1, 2], {}, { valueOf() { return 9 } }, { toString() { return '4' } }, new Date(0)]
let total = 0
for (const x of xs) {
  const n = +x
  total = total + (n === n ? n : 0)
  console.log(typeof n, n, Object.is(n, -0))
}
console.log(total)
const bad: any[] = [Symbol('s'), 10n, { valueOf() { throw new Error('boom') } }]
for (const b of bad) {
  try {
    console.log(+b)
  } catch (e: any) {
    console.log(e instanceof TypeError ? 'TypeError' : e.message)
  }
}
function half(a: any): number {
  return +a / 2
}
console.log(half('9'), half(4), half('q'))
