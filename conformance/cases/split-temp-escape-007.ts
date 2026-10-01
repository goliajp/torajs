// a split product as a branch of a conditional, `??` or `||`
function pick(flag: boolean, a: string, b: string): string[] {
  const r: string[] = flag ? a.split(',') : b.split(';')
  r.push('z' + r.length.toString())
  return r
}
function orEmpty(a: string | null): string[] {
  const r: string[] = a === null ? [] : a.split(',')
  return r
}
function viaNullish(xs: string[] | null, line: string): string[] {
  return xs ?? line.split(',')
}
function viaOr(xs: string[] | undefined, line: string): string[] {
  return xs || line.split(',')
}
function heldBranch(flag: boolean, line: string): string[] {
  const a = line.split(',')
  const b = line.split(';')
  return flag ? a : b
}
const a = 'm,n,o' + (7).toString()
const b = 'u;v' + (8).toString()
console.log(pick(true, a, b).join('|'), pick(false, a, b).join('|'))
console.log(pick(true, a, b)[2], pick(false, a, b)[1])
console.log(orEmpty(a).join('+'), orEmpty(null).length, orEmpty(a)[1])
console.log(viaNullish(null, a).join('+'), viaNullish(['kept' + b], a).join('+'))
console.log(viaOr(undefined, b).join('+'), viaOr(['kept' + a], b)[0])
console.log(heldBranch(true, a + b).join('*'), heldBranch(false, a + b).join('*'))
const t = (a.length > 3 ? a.split(',') : b.split(';')).join('!')
console.log(t)
let w: string[] = []
w = a.length > 100 ? w : b.split(';')
for (const x of w) console.log(x)
console.log(w.length, w[0])
