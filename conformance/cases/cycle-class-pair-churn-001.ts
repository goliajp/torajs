// Two class instances that point at each other die together every
// iteration. Each one leaves the scope buffered as a cycle candidate
// and reaches rc 0 only when the collector trial-deletes the pair, so
// a collect runs every 512 iterations over a buffer whose entries are
// white pairs; the collector must free each pair exactly once while
// the loop keeps allocating through the same free lists.
class Node {
  next: Node | null = null
  prev: Node | null = null
  v: number
  constructor(v: number) { this.v = v }
}
let alive = 0
let keep: Node | null = null
for (let i = 0; i < 200000; i = i + 1) {
  const a = new Node(i)
  const b = new Node(i + 1)
  a.next = b
  b.prev = a
  b.next = a
  a.prev = b
  alive = alive + a.v + b.v
  if ((i & 1023) === 0) keep = a
}
console.log(alive, keep !== null ? keep.v : -1)
