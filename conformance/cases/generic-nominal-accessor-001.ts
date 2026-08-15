// generic-class nominal identity — RFC 20260815-generic-nominal-identity
// blade 3: an instantiation types as `ClassRef("Box<number>")`, so the
// accessor/method tables (keyed by class name) stay reachable, the
// getter's TypeVars substitute through the heritage-argument
// composition, and the read retargets to the monomorphized getter.
class Box<T> {
  private v: T;
  constructor(x: T) { this.v = x; }
  get value(): T { return this.v; }
  describe(): string { return "box:" + this.v; }
}
class NumBox extends Box<number> {
  double(): number { return this.value * 2; }
}
// direct instantiation: accessor + method on the nominal instance
const b = new Box<number>(10);
console.log(b.value);
console.log(b.describe());
const s = new Box<string>("hi");
console.log(s.value);
// inherited: the subclass reads the generic parent's accessor with
// the heritage arguments substituted in, from outside and from a
// method body
const n = new NumBox(21);
console.log(n.value);
console.log(n.double());
console.log(n.describe());
// setter through a generic parent (checker types the write; the
// lowering rides the runtime accessor kernel)
class Cell<T> {
  private raw: T;
  constructor(x: T) { this.raw = x; }
  get held(): T { return this.raw; }
  set held(v: T) { this.raw = v; }
}
class NumCell extends Cell<number> {}
const c = new NumCell(1);
c.held = 5;
console.log(c.held);
