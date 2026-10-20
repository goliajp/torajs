// The room a walk reads at entry is all it trusts: a receiver that
// already holds elements, one handed back with a large buffer and then
// walked over a longer string, and a buffer that grows part way
// through a walk whose later appends still find their slots.
class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}
function lex(src: string, out: Tok[]): Tok[] {
  let i = 0
  while (i < src.length) {
    const c = src.charCodeAt(i)
    if (c === 32) {
      i = i + 1
    } else if (c >= 48 && c <= 57) {
      let v = 0
      while (i < src.length) {
        const d = src.charCodeAt(i)
        if (d < 48 || d > 57) break
        v = v * 10 + (d - 48)
        i = i + 1
      }
      out.push(new Tok(0, v))
    } else {
      out.push(new Tok(1, c))
      i = i + 1
    }
  }
  return out
}
function codes(src: string, out: number[]): number[] {
  let i = 0
  while (i < src.length) {
    const c = src.charCodeAt(i)
    if (c !== 32) {
      out.push(c)
    }
    i = i + 1
  }
  return out
}
function digest(xs: Tok[]): string {
  let a = 0
  let b = 0
  for (let k = 0; k < xs.length; k = k + 1) {
    a = (a * 31 + xs[k].kind + 1) % 1000003
    b = (b * 17 + xs[k].value) % 1000003
  }
  return xs.length + ':' + a + ':' + b
}
const pre = [new Tok(9, 9), new Tok(8, 8)]
console.log(digest(lex('1 + 22 * 333', pre)))
let line = ''
for (let n = 0; n < 40; n = n + 1) {
  line = line + n + ' + x '
  console.log(n, digest(lex(line, [])))
}
const big = lex('7 '.repeat(3000), [])
console.log(digest(big), big[0].value, big[2999].value)
const ns: number[] = [1, 2, 3]
codes('a b c d e f g h i j k l m n o p q r s t u v w x y z'.repeat(20), ns)
let t = 0
for (let k = 0; k < ns.length; k = k + 1) t = t + ns[k] * (k + 1)
console.log(ns.length, t, ns[0], ns[3], ns[ns.length - 1])
const u: any = undefined
try {
  console.log(digest(lex(u, [])))
} catch (e) {
  console.log('caught', e instanceof TypeError)
}
