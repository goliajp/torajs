// RFC 20260706 chunk 573 — cycle-collector color field moved off
// bits 3-4: coloring a buffered frozen class instance Purple (bit 4
// == FLAG_FROZEN) and scanning it back Black used to clear the
// freeze marker, so a post-gc store wrote through (bun throws).
class Node {
  next: Node | null = null;
  tag: number = 0;
}
let a = new Node();
let keep = a;
{
  let b = new Node();
  a.next = b;
  b.next = a;
}
a.tag = 1;
Object.freeze(a);
try {
  a.tag = 50;
} catch (e) {
  console.log("pre-gc throw");
}
console.log(a.tag);
{
  let alias = a;
}
Bun.gc(true);
try {
  a.tag = 99;
} catch (e) {
  console.log("post-gc throw");
}
console.log(a.tag, keep.tag);
