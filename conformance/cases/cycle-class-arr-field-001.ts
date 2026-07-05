// L3b #16 — the cycle collector's array walk descends only into
// ARR_KIND_HEAP arrays. A buffered class instance whose fields
// include scalar arrays (raw i64/f64/bool slots) must not have those
// slots dereferenced as pointers by mark/scan/collect (pre-fix:
// has_walkable_children(1) deref = SIGSEGV during the end-of-main
// drain); heap-element arrays keep walking.

class Node {
  data: number[];
  names: string[];
  next: Node | null;
  constructor(i: number) {
    this.data = [i, i + 1];
    this.names = ["node-name-payload-" + i];
    this.next = null;
  }
}

// two-node cycle — both survive scope drop with rc > 0, register as
// cycle roots, and the exit drain trial-deletes through their fields.
const a = new Node(1);
const b = new Node(2);
a.next = b;
b.next = a;
console.log(a.data[0]);
console.log(b.names[0]);
console.log(a.data[1]);
console.log(b.names.length);

// self-cycle with an f64 array field.
class Ring {
  weights: number[];
  self: Ring | null;
  constructor() {
    this.weights = [1.5, 2.5];
    this.self = null;
  }
}
const r = new Ring();
r.self = r;
console.log(r.weights[1]);
