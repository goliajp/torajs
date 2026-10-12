// A `string | null` (or `string | undefined`) operand of `+` that holds
// a live string is only borrowed by the concat: the binding it came
// from must still read the same text afterwards, and a string built
// right after must not land in its storage.
function bang(x: string | null): string {
  return x + '!'
}
const s = 'ab'.repeat(3)
const r1 = bang(s)
const fresh1 = 'zz'.repeat(3)
console.log(s, r1, fresh1)

const xs: (string | null)[] = ['cd'.repeat(2), null, 'ef'.repeat(2)]
let n = 0
for (let i = 0; i < 3; i = i + 1) {
  n = n + (xs[i] + '?').length
}
const fresh2 = 'yy'.repeat(2)
console.log(n, xs[0], xs[2], fresh2)

function opt(x?: string): string {
  return '<' + x + '>'
}
const o = 'gh'.repeat(2)
console.log(opt(o), opt(), o, 'ww'.repeat(2))

const kv = /([a-z]+)=(\d+)?/g
console.log('k=1;j=;m=22'.replace(kv, (whole, k, v) => '[' + k + '|' + v + ']'))
console.log('k=1;j=;m=22'.replace(kv, (whole, k, v) => v + k + v))
