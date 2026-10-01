// A subclass instance behind a base-typed slot: `class B extends A`
// lays B out as A's fields followed by its own, so the checker admits
// a B anywhere an A is written, and the typed drop of that slot walks
// A's layout. The cell is B's size, one size class up from A's; the
// free must give back the block B was allocated as. The wide cells
// minted afterwards reuse whatever the frees handed back, so a block
// returned to the wrong class shows up as a clobbered neighbour.
class A {
  a: number
  constructor(a: number) {
    this.a = a
  }
}
class B extends A {
  s: string
  p: number
  q: number
  r: number
  constructor(a: number, s: string) {
    super(a)
    this.s = s
    this.p = a + 1
    this.q = a + 2
    this.r = a + 3
  }
}
class Wide {
  a: number
  b: number
  c: number
  d: number
  e: number
  f: number
  constructor(n: number) {
    this.a = n
    this.b = n + 1
    this.c = n + 2
    this.d = n + 3
    this.e = n + 4
    this.f = n + 5
  }
}
class Holder {
  x: A | null = null
}

function first(xs: A[]): number {
  return xs[0].a
}
function mk(n: number): A {
  return new B(n, 'b' + n)
}

const kept: A[] = []
let acc = 0
for (let i = 0; i < 48; i++) {
  // an array anchored on the base, filled with the subclass
  const xs: A[] = []
  xs.push(new B(i, 'x'))
  xs.push(new B(i + 1, 'y'))
  acc += first(xs)
  // a base-typed binding, field, and declared return
  const a: A = new B(2, 'z')
  acc += a.a
  const h = new Holder()
  h.x = new B(3, 'w')
  acc += h.x!.a
  acc += mk(4).a
  kept.push(new A(100 + i))
}
console.log('acc', acc)

const wides: Wide[] = []
for (let i = 0; i < 64; i++) wides.push(new Wide(1000 + i * 10))
let sum = 0
for (let i = 0; i < kept.length; i++) sum += kept[i].a
console.log('kept', sum)
let wsum = 0
for (let i = 0; i < wides.length; i++) wsum += wides[i].a + wides[i].f
console.log('wides', wsum)
console.log(kept[0] instanceof A, kept[47] instanceof A, wides[0] instanceof Wide)
const bs: A[] = [new B(7, 'seven')]
console.log(bs[0] instanceof B, (bs[0] as B).s)
