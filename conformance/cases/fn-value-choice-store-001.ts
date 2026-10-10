// A named function, or a choice among named functions, stored into a
// heap slot typed for function values: array element, object field,
// Map value, directly or through a fn-typed parameter.
function h(x: number): number { return x * 2 }
function inc(x: number): number { return x + 1 }
type F = (x: number) => number
const c = Number("1") > 0

const fs: F[] = []
fs.push(c ? h : inc)
fs.push(h || inc)
fs.push((0, inc))
fs.unshift(c ? inc : h)
console.log(fs.map((f) => f(10)).join(","))
console.log(fs[1] === h, fs[0] === inc)

const lit: F[] = [c ? h : inc, inc]
lit[1] = c ? h : inc
console.log(lit[0](3), lit[1](3))

function take(xs: F[], f: F) { xs.push(f) }
take(fs, c ? inc : h)
take(fs, h)
console.log(fs[4](10), fs[5](10), fs[5] === h)

function put(xs: F[], i: number, f: F) { xs[i] = f }
put(lit, 0, c ? inc : h)
console.log(lit[0](7))

function wrap(f: F): F[] { return [f] }
console.log(wrap(c ? h : inc)[0](8))

const o: { k: F } = { k: c ? inc : h }
console.log(o.k(3))
o.k = c ? h : inc
console.log(o.k(3))
function setK(t: { k: F }, f: F) { t.k = f }
setK(o, inc)
console.log(o.k(4))

class Box {
  k: F
  constructor(k: F) { this.k = k }
}
console.log(new Box(c ? h : inc).k(5), new Box(inc).k(5))

const m = new Map<string, F>()
m.set("a", c ? h : inc)
function setM(mm: Map<string, F>, key: string, f: F) { mm.set(key, f) }
setM(m, "b", c ? inc : h)
console.log(m.get("a")!(5), m.get("b")!(5), m.get("a") === h)

// stored, called many times, then released, over and over
const acc: F[] = []
let s = 0
for (let i = 0; i < 2000; i++) {
  acc.push(i % 3 === 0 ? h : inc)
  take(acc, i % 2 === 0 ? inc : h)
  acc.push((i, h))
  if (acc.length >= 12) {
    for (const f of acc) s += f(i)
    acc.length = 0
  }
}
console.log(s, acc.length)
