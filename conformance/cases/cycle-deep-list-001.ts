// A 200k-node singly linked list built by moving the head pointer:
// every `head = nd` leaves the previous head held only by `nd.next`,
// so each one becomes a cycle candidate and a collect walks from it
// down the whole remaining tail. The walk must not depend on the
// depth of the list: 200k is past where a walk on the machine stack
// overflowed, and inside what the plain release of an acyclic chain
// (one call frame per node) reaches. Built inside a function
// (released when the caller lets go) and as a temporary consumed by a
// single call, with collects forced while the list is still alive.
class Node {
  v: number
  next: Node | null = null
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
    s += h.v % 1000
    h = h.next
  }
  return s
}
function inFunction(n: number): number {
  const h = list(n)
  Bun.gc(true)
  return listSum(h)
}
const N = 200000
console.log(inFunction(N))
console.log(listSum(list(N)))
let held: Node | null = list(N)
Bun.gc(true)
console.log(listSum(held))
held = held.next
Bun.gc(true)
console.log(held === null ? -1 : held.v)
held = null
Bun.gc(true)
console.log(held === null)
