// A derived instance released through a base-typed slot. The base
// layout is a prefix of the derived one, so a release that walked the
// base fields only would leak the derived ones (an array and a heap
// child the base never declared) on every iteration. The per-class
// release reads the instance's own tag and hands a foreign one to the
// runtime's layout walk, which releases all of it.
class Leaf {
  v: number
  constructor(v: number) { this.v = v }
}
class Base {
  head: Leaf | null = null
  sum() { return this.head === null ? 0 : this.head.v }
}
class Derived extends Base {
  tail: Leaf | null = null
  items: number[] = []
  constructor(v: number) {
    super()
    this.tail = new Leaf(v + 1)
    this.items = [v, v, v]
  }
  sum() { return super.sum() + (this.tail === null ? 0 : this.tail.v) + this.items.length }
}
function churn(n: number): number {
  let acc = 0
  for (let i = 0; i < n; i++) {
    const b: Base = new Derived(i)
    b.head = new Leaf(i)
    acc += b.sum()
  }
  return acc
}
console.log(churn(2000))
let keep: Base = new Base()
for (let i = 0; i < 300; i++) {
  keep = i % 2 === 0 ? new Derived(i) : new Base()
}
console.log(keep.sum(), keep instanceof Derived)
