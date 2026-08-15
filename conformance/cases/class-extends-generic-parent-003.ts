// Generic-parent inheritance, generic MIDDLE link: `Wide<U>
// extends Box<U>` re-spells the parent's params in its own, and
// the concrete leaf's heritage argument flows through the chain
// (each flattening hop substitutes exactly one class's params).
class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  get(): T {
    return this.v;
  }
}
class Wide<U> extends Box<U> {
  extra: U;
  constructor(v: U, e: U) {
    super(v);
    this.extra = e;
  }
  both(): U {
    return this.get();
  }
}
class Leaf extends Wide<string> {
  constructor() {
    super("a", "b");
  }
}
const l = new Leaf();
console.log(l.get(), l.extra, l.both(), l instanceof Box, l instanceof Wide);
