// Self-referential classes a thousand deep: a singly linked list and
// a left-leaning tree. The release of the head is a chain of direct
// calls to the same per-class release fn, one frame per node; both
// shapes are built and dropped repeatedly so a frame or a child slot
// left behind would show as a wrong count or a crash.
class Node {
  v: number
  next: Node | null = null
  constructor(v: number) { this.v = v }
}
class Tree {
  v: number
  left: Tree | null = null
  right: Tree | null = null
  constructor(v: number) { this.v = v }
}
function list(n: number): Node {
  let head = new Node(0)
  for (let i = 1; i < n; i++) {
    const nd = new Node(i)
    nd.next = head
    head = nd
  }
  return head
}
function listSum(h: Node | null): number {
  let s = 0
  while (h !== null) {
    s += h.v
    h = h.next
  }
  return s
}
function spine(n: number): Tree {
  let root = new Tree(0)
  for (let i = 1; i < n; i++) {
    const t = new Tree(i)
    t.left = root
    t.right = i % 7 === 0 ? new Tree(-i) : null
    root = t
  }
  return root
}
function treeSum(t: Tree | null): number {
  if (t === null) return 0
  return t.v + treeSum(t.left) + treeSum(t.right)
}
let acc = 0
for (let round = 0; round < 20; round++) {
  const h = list(1000)
  acc += listSum(h)
  const r = spine(1000)
  acc += treeSum(r)
}
console.log(acc)
let held: Node | null = list(1000)
held = held.next
console.log(held === null ? -1 : held.v)
held = null
console.log(held === null)
