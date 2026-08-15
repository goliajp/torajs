// Generic-parent inheritance, multi-param + subclass-own fields:
// the subclass is WIDER than the parent's struct shape, so the
// generic receiver unifies the pattern-width prefix (width
// subtyping over the flattened layout).
class Pair<A, B> {
  a: A;
  b: B;
  constructor(a: A, b: B) {
    this.a = a;
    this.b = b;
  }
  first(): A {
    return this.a;
  }
  second(): B {
    return this.b;
  }
}
class Tagged extends Pair<number, string> {
  tag: string;
  constructor() {
    super(7, "x");
    this.tag = "t";
  }
  show(): string {
    return this.first() + ":" + this.second() + ":" + this.tag;
  }
}
const t = new Tagged();
console.log(t.show(), t.first(), t instanceof Pair);
