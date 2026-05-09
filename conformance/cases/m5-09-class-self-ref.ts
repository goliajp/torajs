// V3-05 — self-referential class field. `next: Node | null`
// is the linked-list shape; before V3-05 the typechecker rejected
// it because the class type wasn't yet in the alias map when the
// field was resolved. Two-phase TypeDecl pass + nominal ClassRef
// placeholder + null-aware default init makes it typecheck and
// lower cleanly. Self-recursive Obj drop routes through
// `__torajs_value_drop_heap` to avoid codegen recursion.

class Node {
  v: number;
  next: Node | null;
  constructor(v: number) { this.v = v; this.next = null; }
}

let a = new Node(1)
let b = new Node(2)
let c = new Node(3)
a.next = b
b.next = c

console.log(a.v)
console.log(a.next === null)
console.log(b.v)
console.log(b.next === null)
console.log(c.v)
console.log(c.next === null)
