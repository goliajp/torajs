// A class whose fields are typed arrays — of its own instances, of
// strings — next to a nullable parent link. The per-class release fn
// walks each array by its element type, the way the inline release
// did: the runtime's tag walk only learns an array's element kind
// once it crossed into the any world, and these never do, so handing
// them to it would free the array block with every element held.
class Node {
  kids: Node[] = []
  parent: Node | null = null
  names: string[] = []
  v: number
  constructor(v: number) { this.v = v }
}
function tree(i: number): Node {
  const root = new Node(i)
  for (let k = 0; k < 3; k++) {
    const kid = new Node(i * 10 + k)
    kid.names.push("k" + k, "n" + i)
    root.kids.push(kid)
  }
  root.names.push("root")
  return root
}
function weigh(n: Node): number {
  let s = n.v + n.names.length
  for (const k of n.kids) s += weigh(k)
  return s
}
let acc = 0
for (let i = 0; i < 2000; i++) {
  const t = tree(i)
  acc += weigh(t)
}
console.log(acc)
let linked = 0
for (let i = 0; i < 1000; i++) {
  const p = new Node(i)
  const c = new Node(i + 1)
  c.parent = p
  p.kids.push(c)
  linked += c.parent === p ? 1 : 0
}
console.log(linked)
const keep = tree(5)
keep.kids = [new Node(9)]
keep.names = []
console.log(weigh(keep), keep.kids.length, keep.names.length)
