// A cell borrowed through a binding (`old.child`, `old[0]`, `old || x`)
// and handed to a consumer is taken only after the later operands run;
// a later operand that releases its source must not leave the consumer
// holding a freed cell.
class N {
  v: number
  w: number
  x: number
  y: number
  child: N | null
  constructor(v: number, child: N | null) {
    this.v = v
    this.w = v
    this.x = v
    this.y = v
    this.child = child
  }
  m(b: number): number {
    return this.v * 10 + b
  }
}
class H {
  k: N
  ks: N[]
  constructor(i: number) {
    this.k = new N(i, new N(i + 1, null))
    this.ks = [new N(i + 2, null)]
  }
}
const keep: N[] = []
const fallback = new N(-1, null)

function use(a: N | null, b: number): number {
  return (a === null ? -100 : a.v) * 10 + b
}

function reset(h: H): number {
  h.k = new N(500, null)
  h.ks = [new N(600, null)]
  keep.push(new N(66, null))
  return 1
}

function viaOr(h: H): number {
  const old = h.k
  return use(old || fallback, reset(h))
}

function viaAnd(h: H): number {
  const old = h.k
  return use(old && old, reset(h))
}

function viaChild(h: H): number {
  const old = h.k
  return use(old.child, reset(h))
}

function viaOptChild(h: H): number {
  const old: N | null = h.k
  return use(old?.child ?? null, reset(h))
}

function viaIndex(h: H): number {
  const old = h.ks
  return use(old[0], reset(h))
}

function viaMethod(h: H): number {
  const old = h.k
  return old.child!.m(reset(h))
}

let s = 0
for (let i = 0; i < 100; i++) {
  s += viaOr(new H(i))
  s += viaAnd(new H(i))
  s += viaChild(new H(i))
  s += viaOptChild(new H(i))
  s += viaIndex(new H(i))
  s += viaMethod(new H(i))
}
console.log(s, keep.length)
