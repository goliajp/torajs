// r499 — main's end-of-program drains on demand, the cycle side: a
// promise-free program whose objects form a reference cycle keeps
// the exit-time cycle drain (the cycle member's text is live through
// the class drop path), while the microtask drain and the rejection
// sweep are elided. Output must not depend on which drains remain.
class Node {
  next: Node | null = null;
  constructor(public v: number) {}
}
function ring(n: number): Node {
  const head = new Node(0);
  let cur = head;
  for (let i = 1; i < n; i++) {
    const nx = new Node(i);
    cur.next = nx;
    cur = nx;
  }
  cur.next = head;
  return head;
}
let total = 0;
for (let r = 0; r < 50; r++) {
  let p: Node | null = ring(8);
  for (let i = 0; i < 8; i++) {
    total += p!.v;
    p = p!.next;
  }
}
console.log(total);
