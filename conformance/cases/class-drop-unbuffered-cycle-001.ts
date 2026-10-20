// A node's field written after its constructor returned can close a
// cycle: the release must keep buffering a node whose count stays
// above zero, and the collector must free each two-node ring.
class Node {
  id: number
  next: Node | null
  constructor(id: number, next: Node | null) {
    this.id = id
    this.next = next
  }
}
let sum = 0
let last: Node | null = null
for (let i = 0; i < 20000; i++) {
  const a = new Node(i, null)
  const b = new Node(i + 1, a)
  a.next = b
  sum = (sum + a.next.next!.id + b.next!.next!.id) | 0
  if (i % 1000 === 0) last = b
}
console.log(sum, last!.id, last!.next!.next!.id)
last = null
console.log(last === null)
