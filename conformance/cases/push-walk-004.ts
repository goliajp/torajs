// Two appends a trip, in sequence and in one arm only.
class Tok {
  k: number
  v: number
  constructor(k: number, v: number) {
    this.k = k
    this.v = v
  }
}
function twice(s: string): number[] {
  const out: number[] = []
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    out.push(c)
    out.push(-c)
    i = i + 1
  }
  return out
}
function some(s: string): Tok[] {
  const out: Tok[] = [new Tok(9, 9)]
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c > 100) {
      out.push(new Tok(c, 0))
      out.push(new Tok(0, c))
    } else {
      out.push(new Tok(c, c))
    }
    i = i + 1
  }
  return out
}
console.log(twice('abc').join(','), twice('').length)
const t = some('zaéy')
let r = ''
for (let k = 0; k < t.length; k = k + 1) r = r + t[k].k + '/' + t[k].v + ' '
console.log(t.length, r)
const big = twice('w'.repeat(3000))
console.log(big.length, big[5999], big[2998])
