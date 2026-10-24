// A value read out of a binding a closure can reassign, or out of a
// place that code the runtime runs can clear, stays alive for the
// whole call; a value no code can free during the call is read as it
// was.
class P {
  pos: number
  tag: number
  constructor(pos: number) {
    this.pos = pos
    this.tag = 0
  }
}
class Q {
  a: number
  b: number
  constructor(a: number, b: number) {
    this.a = a
    this.b = b
  }
}
let keep: Q | null = null

// a local shared with a closure that the callee runs
function use(p: P, f: () => void): number {
  f()
  keep = new Q(1, 999)
  return p.pos
}
function run(): number {
  let o: P | null = new P(7)
  const clear = () => {
    o = null
  }
  return use(o!, clear)
}
for (let i = 0; i < 3; i = i + 1) {
  console.log("closure", run())
}

// top-level bindings reassigned by destructuring and by for-of
let gp: P = new P(20)
function viaDestructure(p: P): number {
  ;[gp] = [new P(1)]
  keep = new Q(2, 998)
  return p.pos
}
console.log("destructure", viaDestructure(gp))
let fp: P = new P(30)
function viaForOf(p: P): number {
  for (fp of [new P(2)]) {
  }
  keep = new Q(3, 997)
  return p.pos
}
console.log("for-of", viaForOf(fp))

// a getter on the array prototype, run by a read at a key that is not
// an index, clears the place the argument came from
class Holder {
  p: P | null = null
}
const holder = new Holder()
Object.defineProperty(Array.prototype, "1.5", {
  get() {
    holder.p = null
    keep = new Q(4, 996)
    return 5
  },
  configurable: true,
})
function readKey(p: P, xs: number[], k: number): number {
  const v = xs[k]
  return p.pos + v
}
holder.p = new P(40)
console.log("getter", readKey(holder.p!, [1, 2], 1.5))

// a tree walked through its fields: nothing frees a node during the walk
class N {
  v: number
  l: N | null
  r: N | null
  constructor(v: number, l: N | null, r: N | null) {
    this.v = v
    this.l = l
    this.r = r
  }
}
function build(d: number): N {
  if (d === 0) return new N(1, null, null)
  return new N(d, build(d - 1), build(d - 1))
}
function sum(n: N, xs: number[]): number {
  let s = n.v + xs[n.v & 1]
  if (n.l !== null) s = s + sum(n.l!, xs)
  if (n.r !== null) s = s + sum(n.r!, xs)
  return s
}
console.log("tree", sum(build(6), [10, 20]))
