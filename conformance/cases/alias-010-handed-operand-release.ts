// A borrowed binding handed to a consumer — an argument, an array
// element — is taken only after every later operand has run, so a
// later operand that releases its source must not leave the consumer
// holding a freed cell.
class N {
  v: number
  w: number
  x: number
  y: number
  constructor(v: number) {
    this.v = v
    this.w = v
    this.x = v
    this.y = v
  }
}
class H {
  k: N
  constructor() {
    this.k = new N(1)
  }
}
const keep: N[] = []

function use(a: N, b: number): number {
  return a.v * 10 + b
}

function reset(h: H): number {
  h.k = new N(5)
  keep.push(new N(66))
  return 1
}

function mk(h: H): N {
  h.k = new N(6)
  keep.push(new N(67))
  return new N(2)
}

function viaArgs(h: H): number {
  const old = h.k
  return use(old, reset(h))
}

function viaArray(h: H): number {
  const old = h.k
  const a: N[] = [old, mk(h)]
  keep.push(new N(68))
  return a[0].v + a[1].v
}

let s = 0
for (let i = 0; i < 200; i++) {
  s += viaArgs(new H())
  s += viaArray(new H())
}
console.log(s, keep.length)
