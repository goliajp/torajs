// The cycle collector must not dereference an `any`-typed class field
// as a raw pointer: the slot holds a NaN-box, so a boxed immediate
// (`this.v = 1` → 0xFFFE…0001) is not a heap address.
//
// `Type::Any` is refcounted, so the emit side records an `any` field's
// offset in the class's `child_offsets` — correctly, since the slot CAN
// hold a cell. Every other consumer of those offsets already gates on
// the NaN-box cell-like predicate (rc_inc / rc_dec /
// __torajs_value_drop_heap); the collector's walk phases read the slot
// raw, dereferenced the immediate as a header, and decremented through
// it. The walk only runs once 1024 candidates have buffered, so this
// needs churn past that threshold to reproduce — under it, the loops
// below merely allocate.
//
// A candidate is buffered when a class instance with pointer-capable
// children survives an rc decrement, so each shape here keeps one
// reference alive (the array) while another (the binding) is released.

class Holder {
  v: any;
  n: number;
  constructor(v: any) {
    this.v = v;
    this.n = 0;
  }
}

// Boxed immediates in the `any` slot — the shape that SIGSEGV'd.
const nums: any[] = [];
for (let i = 0; i < 2000; i++) {
  const h = new Holder(1);
  nums.push(h);
}
console.log(nums.length); // 2000

// Boxed ShortStr / Bool — the other immediate encodings the raw read
// also mistook for pointers.
const strs: any[] = [];
for (let i = 0; i < 2000; i++) {
  const h = new Holder("ab");
  strs.push(h);
}
console.log(strs.length); // 2000

const bools: any[] = [];
for (let i = 0; i < 2000; i++) {
  const h = new Holder(true);
  bools.push(h);
}
console.log(bools.length); // 2000

// A real heap cell in the `any` slot still walks: the collector must
// keep descending into genuine children, not skip every `any` field.
class Node {
  next: any;
  constructor() {
    this.next = null;
  }
}
const nodes: any[] = [];
for (let i = 0; i < 2000; i++) {
  const a = new Node();
  const b = new Node();
  a.next = b;
  nodes.push(a);
}
console.log(nodes.length); // 2000
console.log(nodes[1999].next.next); // null
