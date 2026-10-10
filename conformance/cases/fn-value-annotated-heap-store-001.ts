// A fn-typed binding stored into the heap holds a closure cell: an
// annotated const initialized with a named function or a choice among
// them, an unannotated choice, an alias of another such binding, an
// alias of a fn-typed parameter, and a choice between a named function
// and a parameter.
type F = (x: number) => number
function h(x: number): number { return x * 2 }
function inc(x: number): number { return x + 1 }

const g: F = h
const fs: F[] = [g]
console.log(fs[0](10))

function chooseAnn(c: boolean): number {
  const k: F = c ? h : inc
  const xs: F[] = []
  xs.push(k)
  return xs[0](3)
}
console.log(chooseAnn(true), chooseAnn(false))

function chooseBare(c: boolean): number {
  const k = c ? h : inc
  const xs: F[] = []
  xs.push(k)
  return xs[0](3)
}
console.log(chooseBare(true), chooseBare(false))

const g2: F = inc
const m = new Map<string, F>()
m.set("a", g2)
console.log(m.get("a")!(5))
const o: { f: F } = { f: g2 }
console.log(o.f(7))
const ys: F[] = [h]
ys[0] = g2
console.log(ys[0](1), g2(2))

function take(xs: F[], f: F): void { xs.push(f) }
const viaParam: F[] = []
take(viaParam, g)
console.log(viaParam[0](4))

function withLiteral(c: boolean): F[] {
  const k: F = c ? h : (x: number) => x - 1
  return [k, k]
}
console.log(withLiteral(true)[1](9), withLiteral(false)[0](9))

const viaOther: { f: F }[] = []
const e: F = process.argv.length > 100 ? h : inc
viaOther.push({ f: e })
console.log(viaOther[0].f(30))
const set = new Set<F>()
set.add(e)
console.log(set.size, set.has(e))

const chained: F = g
const cs: F[] = [chained]
console.log(cs[0](5), g(1), chained(2))

function aliasParam(f: F, xs: F[]): void { const k: F = f; xs.push(k) }
const ap: F[] = []
aliasParam(h, ap)
console.log(ap[0](5))

function shadowed(h: F, xs: F[]): void { const k: F = h; xs.push(k) }
const sh: F[] = []
shadowed(inc, sh)
console.log(sh[0](5))

function mixed(c: boolean, f: F, zs: F[]): void { const k: F = c ? h : f; zs.push(k) }
const mz: F[] = []
mixed(true, inc, mz)
mixed(false, inc, mz)
console.log(mz[0](5), mz[1](5))

function mixedDirect(c: boolean, f: F, zs: F[]): void { zs.push(c ? h : f) }
const md: F[] = []
mixedDirect(true, inc, md)
mixedDirect(false, inc, md)
console.log(md[0](6), md[1](6))

function local(): number {
  const k: F = inc
  return k(1) + k(2)
}
console.log(local())

function pushNamed(zs: F[]): void { zs.push(h) }
function pushChoice(zs: F[], c: boolean): void { zs.push(c ? h : (x: number) => x) }
function setNamed(zs: F[]): void { zs[0] = inc }
const pz: F[] = []
pushNamed(pz)
pushChoice(pz, true)
pushChoice(pz, false)
console.log(pz[0](3), pz[1](3), pz[2](3))
setNamed(pz)
console.log(pz[0](3))
