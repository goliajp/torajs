// A release on a branch that leaves by `throw` or `return` still
// reaches the `catch` / `finally` around it: a field read borrowed
// before the `try` must survive into either.
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
type J = { v: number; w: number; x: number; y: number }
const junk: J[] = []
const log: number[] = []

function viaCatch(h: H, c: boolean): number {
  const old = h.k
  try {
    if (c) {
      h.k = new N(2)
      throw 0
    }
  } catch (e) {
    junk.push({ v: 77, w: 77, x: 77, y: 77 })
    return old.v
  }
  return -1
}

function viaFinally(h: H, c: boolean): number {
  const old = h.k
  try {
    if (c) {
      h.k = new N(3)
      return 1
    }
  } finally {
    junk.push({ v: 78, w: 78, x: 78, y: 78 })
    log.push(old.v)
  }
  return 0
}

let s = 0
for (let i = 0; i < 200; i++) {
  const h = new H()
  s += viaCatch(h, true)
  s += viaFinally(h, true)
}
let t = 0
for (const x of log) t += x
console.log(s, t, junk.length)
