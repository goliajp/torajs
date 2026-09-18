// K5-3 — a program that re-links prototypes reads every `super` base
// when the site runs; a per-evaluation family reads it off ITS OWN
// evaluation's home, so re-linking one evaluation leaves the other.
function mk(): any {
  class A { who() { return "A" } static tag() { return "sA" } }
  class B extends A {
    who() { return "B>" + super.who() }
    static tag() { return "sB>" + super.tag() }
    static self() { return this }
  }
  return [A, B]
}
const [A1, B1] = mk(), [A2, B2] = mk();
const X = { who() { return "X" } };
const SX = { tag() { return "sX" } };
const b1: any = new B1(), b2: any = new B2();
console.log(b1.who(), b2.who(), B1.tag(), B2.tag());
Object.setPrototypeOf(B1.prototype, X);
// (constructing B1 after this line is a TypeError: `super(…)` looks the
// parent constructor up on B1 itself)
Object.setPrototypeOf(B1, SX);
console.log(b1.who(), b2.who(), B1.tag(), B2.tag());
console.log(B1.self() === B1, b2 instanceof A2, b1 instanceof A1, b1 instanceof B1, A1 !== A2);
