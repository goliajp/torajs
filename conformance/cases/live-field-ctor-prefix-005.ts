// A call whose callee name a parameter shadows is not a call of the
// declared function of that name: `mkLeaf()` below runs the closure
// the caller passed, which hands back an element read past the end,
// so the field holds undefined and the next member read throws
// (§13.3.2.1 step 3 via ToObject).
class Leaf {
  v: number
  constructor(v: number) {
    this.v = v
  }
}
class Box {
  item: Leaf
  constructor(item: Leaf) {
    this.item = item
  }
}
function name(e: unknown): string {
  return e instanceof TypeError ? "TypeError" : "other"
}
function mkLeaf(): Leaf {
  return new Leaf(5)
}
function set(b: Box, mkLeaf: () => Leaf) {
  b.item = mkLeaf()
}
const xs: Leaf[] = []
const b = new Box(new Leaf(1))
set(b, () => xs[0])
try {
  console.log(b.item.v)
} catch (e) {
  console.log("parameter:", name(e))
}
console.log(mkLeaf().v)
