// A class whose layout holds an object field releases its instances
// through one out-of-line body, and that body releases the field by
// the field's declared class. A subclass instance behind that field
// is one size class up from the base; a wider object literal behind
// an anonymous-typed field likewise. Both frees have to give back the
// block the cell was allocated as. The wide cells minted afterwards
// reuse whatever the frees handed back, so a block returned to the
// wrong class shows up as a clobbered neighbour.
class Node {
  a: number
  next: Node | null = null
  constructor(a: number) {
    this.a = a
  }
}
class FatNode extends Node {
  p: number
  q: number
  r: number
  s: number
  constructor(a: number) {
    super(a)
    this.p = a + 1
    this.q = a + 2
    this.r = a + 3
    this.s = a + 4
  }
}
class Holder {
  head: Node | null = null
  tag: { k: number } | null = null
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

const kept: Node[] = []
let acc = 0
for (let i = 0; i < 48; i++) {
  // the subclass behind the base-typed field, and behind the base's
  // own self-typed field
  const h = new Holder()
  h.head = new FatNode(i)
  h.head.next = new FatNode(i + 1)
  acc += h.head.a + h.head.next!.a
  // a wider literal behind an anonymous-typed field
  h.tag = { k: i, l: 1, m: 2, n: 3, o: 4 }
  acc += h.tag.k
  kept.push(new Node(100 + i))
}
console.log('acc', acc)

// wide cells now: each takes a block the frees above handed back; a
// block of the wrong class is overrun into its neighbour, and the
// cells minted after the wides overrun back into them
const wides: Wide[] = []
for (let i = 0; i < 64; i++) wides.push(new Wide(1000 + i * 10))
const kept2: Node[] = []
for (let i = 0; i < 48; i++) kept2.push(new Node(200 + i))
let sum = 0
let okKept = 0
for (let i = 0; i < kept.length; i++) {
  sum += kept[i].a
  if (kept[i] instanceof Node) okKept++
}
for (let i = 0; i < kept2.length; i++) {
  sum += kept2[i].a
  if (kept2[i] instanceof Node) okKept++
}
console.log('kept', sum, okKept)
let wsum = 0
let okWide = 0
for (let i = 0; i < wides.length; i++) {
  wsum += wides[i].a + wides[i].e + wides[i].f
  if (wides[i] instanceof Wide) okWide++
}
console.log('wides', wsum, okWide)
const h = new Holder()
h.head = new FatNode(7)
console.log(h.head instanceof FatNode, (h.head as FatNode).s)
