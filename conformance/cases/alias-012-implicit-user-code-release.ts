// Operators and reads reach user code on their own — a `valueOf` behind
// unary `+` or `+`, a getter installed by `defineProperty` — and that
// code may release what a borrowed binding still names.
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
const g = new H()

class V {
  valueOf(): number {
    g.k = new N(3)
    keep.push(new N(44))
    return 1
  }
}

function viaUnaryPlus(v: V): number {
  const old = g.k
  const n = +v
  return old.v + n
}

function viaAdd(v: any): number {
  const old = g.k
  const n = 1 + v
  return old.v + n
}

const o: any = {}
Object.defineProperty(o, 'p', {
  get() {
    g.k = new N(4)
    keep.push(new N(45))
    return 1
  },
})

function viaGetter(q: any): number {
  const old = g.k
  const n = q.p
  return old.v + n
}

let s = 0
for (let i = 0; i < 100; i++) {
  g.k = new N(1)
  s += viaUnaryPlus(new V())
  g.k = new N(1)
  s += viaAdd(new V())
  g.k = new N(1)
  s += viaGetter(o)
}
console.log(s, keep.length)
