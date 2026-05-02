// User class defines a method whose name (`push`) collides with a
// known Array built-in. The class body internally calls
// `this.<arr_field>.push(v)` — without the desugar guard, that would
// rewrite to `__cm_C__push(this.data, v)` and infinite-recurse.
//
// desugar_classes detects the shape (receiver is `this.<field>` and
// the field's type ann is `T[]` / `string` / `number`) and skips the
// single-owner rewrite, leaving the Member call shape so ssa_lower's
// type-aware Array.push intrinsic dispatch picks it up.

class Stack<T> {
  data: T[];
  constructor(seed: T) {
    let init: T[] = [];
    this.data = init;
    this.data.push(seed);  // ← Array.push, NOT Stack.push
  }
  push(v: T): void {
    this.data.push(v);     // ← Array.push (inside Stack.push body)
  }
  size(): number {
    return this.data.length;
  }
}

function check(): number {
  let s = new Stack(1);
  s.push(2);                 // Stack.push (call site picked by type-aware dispatch)
  s.push(3);
  s.push(4);
  if (s.size() !== 4) { throw "#1: stack size after pushes"; }
  return 0;
}
console.log(check());
