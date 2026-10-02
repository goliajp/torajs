// An element store whose index is a non-integer-typed number (`i % 2`)
// keeps its value alive in the array, whether the assignment's own
// value is discarded or kept.
class N {
  a: number
  constructor(n: number) {
    this.a = n
  }
}

const ys: N[] = [new N(0), new N(1)]
for (let i = 1; i <= 1000; i++) ys[i % 2] = new N(i)
console.log(ys[0].a, ys[1].a)

function setAt(xs: N[], i: number, n: number): void {
  xs[i] = new N(n)
}
const zs: N[] = [new N(0), new N(1), new N(2)]
for (let i = 1; i <= 1000; i++) setAt(zs, i % 3, i)
console.log(zs[0].a, zs[1].a, zs[2].a)

const ws: N[] = [new N(0), new N(1)]
const kept: N[] = []
for (let i = 1; i <= 1000; i++) kept.push((ws[i % 2] = new N(i * 2)))
const junk: N[] = []
for (let i = 0; i < 2000; i++) junk.push(new N(100000 + i))
let total = 0
for (const x of kept) total += x.a
console.log(ws[0].a, ws[1].a, total, junk.length)

const anys: any[] = [new N(0), new N(1)]
for (let i = 1; i <= 1000; i++) anys[i % 2] = new N(i)
console.log(anys[0].a, anys[1].a)
