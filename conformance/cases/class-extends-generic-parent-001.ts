// Generic-parent inheritance, base shape: the heritage type
// arguments substitute into the parent's flattened fields
// (`v: T` → `v: number`), and the inherited generic methods
// unify their ClassRef receiver against the parent's struct
// shape (rotation 411, monomorphization twin wiring).
class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  get(): T {
    return this.v;
  }
}
class NumBox extends Box<number> {
  constructor() {
    super(42);
  }
  double(): number {
    return this.get() * 2;
  }
}
const b = new NumBox();
console.log(b.get(), b.double(), b instanceof Box);
