// A user function spelled like a synthesized per-class release fn
// (`__drop_<C>` for the class Box below) is an ordinary function: the
// retain that precedes a call to it stays, its calls are never taken
// for a release, and the class's own release still runs. The value
// read after the call proves the retain was kept.
class Leaf {
  v: number
  constructor(v: number) { this.v = v }
}
class Box {
  leaf: Leaf | null = null
  tag: number
  constructor(tag: number) { this.tag = tag }
}
function __drop_Box(b: Box): number {
  return b.leaf === null ? 0 : b.leaf.v + b.tag
}
function __drop_x(p: Leaf): number {
  return p.v * 2
}
function hold(b: Box): Box {
  __drop_Box(b)
  return b
}
let acc = 0
for (let i = 0; i < 3000; i++) {
  const b = new Box(i)
  b.leaf = new Leaf(i)
  const same = hold(b)
  const leaf = b.leaf
  acc += __drop_Box(same) + (leaf === null ? 0 : __drop_x(leaf))
  acc += same.leaf === null ? 0 : same.leaf.v
}
console.log(acc)
const b = new Box(7)
const l = new Leaf(8)
b.leaf = l
console.log(__drop_Box(b), __drop_Box(b), l.v, __drop_x(l))
