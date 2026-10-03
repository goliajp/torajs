class Tok { constructor(public v: number) {} }
class D {
  last(toks: Tok[]): number { return toks[toks.length - 1].v + toks[0].v * 100 + toks.length * 10000 }
}
const toks: Tok[] = []
for (let i = 0; i < 5; i = i + 1) toks.push(new Tok(i + process.argv.length))
toks.shift()
const d = new D()
console.log(d.last(toks), toks[toks.length - 1].v + toks[0].v * 100 + toks.length * 10000)
function f(xs: number[]): number { return xs[0] * 100 + xs[xs.length - 1] + xs.length * 10000 }
const xs = [1, 2, 3, 4, 5]
xs.shift()
console.log(f(xs), xs[0] * 100 + xs[xs.length - 1] + xs.length * 10000)
const q = [10, 20, 30, 40]
function g() { q.shift() }
g()
const i = process.argv.length - 2
console.log(q[i], q[i + 1], q.length)
function h(o: { a: number[] }): number { const a = o.a; return a[i] }
const o = { a: [5, 6, 7] }
o.a.shift()
console.log(h(o), o.a[i])
function mk(): number[] { const r = [7, 8, 9]; r.shift(); return r }
const m = mk()
console.log(m[i])
// fresh arrays this body made keep the head-free read
const fr = [1, 2, 3].map((v) => v * 2)
const sp = 'a,b,c'.split(',')
console.log(fr[i], sp[i + 1], fr.length + sp.length)
function local(): number {
  const own = [3, 4, 5]
  own.shift()
  const viaCall = [6, 7, 8]
  g2(viaCall)
  return own[i] * 100 + viaCall[i]
}
function g2(a: number[]) { a.shift() }
console.log(local())
