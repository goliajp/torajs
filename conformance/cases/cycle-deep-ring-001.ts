// Cyclic structures a million nodes deep, which only the cycle
// collector can reclaim: a singly linked ring (the tail points back
// at the head) and a doubly linked list (every adjacent pair is a
// cycle). The collector has to trial-delete, scan and sweep the whole
// structure in one walk, first while it is still referenced (every
// node survives), then once the last outside reference is gone.
class Node {
  v: number
  next: Node | null = null
  prev: Node | null = null
  constructor(v: number) { this.v = v }
}
function ring(n: number): Node {
  const tail = new Node(0)
  let head = tail
  for (let i = 1; i < n; i++) {
    const nd = new Node(i)
    nd.next = head
    head = nd
  }
  tail.next = head
  return head
}
function doubly(n: number): Node {
  let head = new Node(0)
  for (let i = 1; i < n; i++) {
    const nd = new Node(i)
    nd.next = head
    head.prev = nd
    head = nd
  }
  return head
}
function ringSum(h: Node): number {
  let s = h.v % 1000
  let p = h.next
  while (p !== null && p !== h) {
    s += p.v % 1000
    p = p.next
  }
  return s
}
function backSum(h: Node): number {
  let last: Node | null = null
  let q: Node | null = h
  while (q !== null) {
    last = q
    q = q.next
  }
  let s = 0
  let p: Node | null = last
  while (p !== null) {
    s += p.v % 1000
    p = p.prev
  }
  return s
}
const N = 1000000
let r: Node | null = ring(N)
Bun.gc(true)
console.log(ringSum(r))
r = null
Bun.gc(true)
let d: Node | null = doubly(N)
Bun.gc(true)
console.log(backSum(d))
d = null
Bun.gc(true)
for (let round = 0; round < 3; round++) {
  console.log(ringSum(ring(N / 10)) + backSum(doubly(N / 10)))
}
console.log(r === null && d === null)
