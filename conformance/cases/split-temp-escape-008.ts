// a split product spread into a literal, a call or a push
function count(...xs: string[]): string {
  return xs.length.toString() + ':' + xs.join('')
}
function spreadRet(s: string): string[] {
  return [...s.split(','), 'last' + s]
}
const s = 'r,s,t' + (9).toString()
const sp: string[] = [...s.split(',')]
sp.push('u' + sp.length.toString())
console.log(sp.join('|'), sp[2], sp.length)
const both = ['h' + s, ...s.split(','), ...s.split('s')]
console.log(both.join('|'), both[1], both[5], both.length)
console.log(count(...s.split(',')))
const acc: string[] = []
acc.push(...s.split(','))
acc.push(...s.split('t'))
console.log(acc.join('+'), acc[4], acc.length)
const r = spreadRet(s)
for (const x of r) console.log(x)
console.log(Array.from(s.split(',')).join('?'))
console.log(s.split(',').concat(s.split('r')).join('&'))
console.log([s.split(',')].length, [s.split(',')][0][1])
