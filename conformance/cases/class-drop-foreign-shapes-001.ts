// Cells that reach a class-typed slot without being minted by that
// class: a class-shaped literal, a generic specialization, a cell
// that went through `any` and back, and an instance a WeakRef
// observes while it dies through the per-class release fn.
class Leaf {
  v: number
  constructor(v: number) { this.v = v }
}
class Pair {
  left: Leaf | null = null
  right: Leaf | null = null
}
class Gen<T> {
  item: T
  link: Leaf | null = null
  constructor(item: T) { this.item = item }
}
function sum(p: Pair): number {
  return (p.left === null ? 0 : p.left.v) + (p.right === null ? 0 : p.right.v)
}
let acc = 0
for (let i = 0; i < 2000; i++) {
  const lit: Pair = { left: new Leaf(i), right: null }
  const p = new Pair()
  p.left = new Leaf(1)
  p.right = new Leaf(2)
  const a: any = p
  const back = a as Pair
  const g = new Gen<number>(i)
  g.link = new Leaf(3)
  acc += sum(lit) + sum(back) + g.item + (g.link === null ? 0 : g.link.v)
}
console.log(acc)
let seen = 0
for (let i = 0; i < 200; i++) {
  const p = new Pair()
  p.left = new Leaf(i)
  const w = new WeakRef(p)
  const d = w.deref()
  seen += d === undefined ? 0 : sum(d)
}
console.log(seen)
