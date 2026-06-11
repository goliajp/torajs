// D5 (ann-width rfc §5.4) — cyclic type-alias field widths join at
// type granularity through the alias's reserved layout; fract writes
// through any depth must read back exactly (pre-D5: compile abort).

// c2 — shallow fract write
type Item = { v: number; next: Item | null };
const c: Item = { v: 1, next: null };
c.v = 0.25;
console.log(c.v);

// deep write through the recursion
c.next = { v: 2, next: null };
const tail = c.next;
if (tail !== null) {
  tail.v = 0.5;
  console.log(tail.v);
}

// int-hold — all-int cyclic alias keeps the narrow layout
type Counter = { n: number; next: Counter | null };
const k: Counter = { n: 1, next: null };
k.n = 7;
console.log(k.n);

// class self-reference (c1) holds
class Node {
  val: number;
  next: Node | null;
  constructor(v: number) {
    this.val = v;
    this.next = null;
  }
}
const nd = new Node(1);
nd.val = 0.5;
console.log(nd.val);

// mixed — class field typed by a cyclic alias
class Holder {
  it: Item | null;
  constructor() {
    this.it = null;
  }
}
const h = new Holder();
h.it = c;
const got = h.it;
if (got !== null) {
  console.log(got.v);
}

// mixed — cyclic alias holding a class-typed field
type Wrap = { w: number; node: Node | null; next: Wrap | null };
const wr: Wrap = { w: 1, node: nd, next: null };
wr.w = 2.5;
console.log(wr.w);
console.log(wr.node !== null ? 1 : 0);

// mutual recursion
type A = { x: number; b: B | null };
type B = { y: number; a: A | null };
const aa: A = { x: 1, b: null };
aa.x = 1.5;
console.log(aa.x);

// param-annotation flow into the nominal join
function bump(it: Item): void {
  it.v = 0.125;
}
bump(c);
console.log(c.v);
