// A constructor that hands `this` to another node's constructor before
// it is done: the child points back at the parent, so a parent and its
// child form a ring. The release keeps the cycle buffer.
class Node {
  id: number
  link: Node | null
  constructor(id: number, back: Node | null, spawn: boolean) {
    this.id = id
    this.link = back
    if (spawn) this.link = new Node(id * 10, this, false)
  }
}
let sum = 0
for (let i = 0; i < 20000; i++) {
  const p = new Node(i, null, true)
  sum = (sum + p.link!.id + p.link!.link!.id) | 0
}
const q = new Node(7, null, true)
console.log(sum, q.link!.link === q, q.link!.id)
