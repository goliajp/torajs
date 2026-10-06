// A concat whose result is only measured folds into the sum of its
// operands' lengths; a null operand still reads as the text "null"
// (4 code units) and an unmatched capture group as "undefined".
const a: (string | null)[] = ['ab', null, 'cde']
let t = 0
for (let i = 0; i < 3; i = i + 1) {
  t = t + (a[i] + 'x').length
}
console.log(t)
const m = /(a)(b)?/.exec('ac')
let u = 0
if (m !== null) {
  u = u + (m[2] + 'x').length
  u = u + (m[1] + 'y').length
}
console.log(u)
function f(x: string | null): number {
  return (x + '!').length
}
console.log(f(a[1]) + f('zz') + f(a[0]))
let acc = ''
let seen = 0
for (let i = 0; i < 20; i = i + 1) {
  acc = acc + 'ab'
  seen = seen + (acc + a[i % 3]).length
}
console.log(seen, acc.length)
