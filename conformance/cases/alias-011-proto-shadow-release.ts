// A built-in method the program has replaced runs user code: a call
// to it may release what a borrowed binding still names.
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
;(Array.prototype as any).indexOf = function (v: any): number {
  g.k = new N(2)
  keep.push(new N(55))
  return 0
}

function f(xs: number[]): number {
  const old = g.k
  const i = xs.indexOf(3)
  return old.v + i
}

let s = 0
for (let i = 0; i < 200; i++) {
  g.k = new N(1)
  s += f([1, 2, 3])
}
console.log(s, keep.length)
