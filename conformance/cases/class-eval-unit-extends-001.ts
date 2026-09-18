// K5-1 — a class that extends a per-evaluation class of the same
// container is a per-evaluation class too (§15.7.14): each evaluation
// chains ITS constructor under that evaluation's parent constructor and
// its prototype under that parent's prototype, so `instanceof`, static
// inheritance and `super` all answer per evaluation.
function mk(): any {
  class A {
    x = 1;
    constructor(public tag: string) {}
    a() { return "a" + this.x; }
    who() { return "A"; }
    static s() { return this; }
    static make() { return new this("m"); }
  }
  class B extends A {
    y = 2;
    constructor() { super("b"); }
    who() { return "B>" + super.who(); }
    sum() { return this.x + this.y; }
    static t() { return super.s(); }
  }
  class C extends B {
    who() { return "C>" + super.who(); }
  }
  return [A, B, C];
}
const r1 = mk(), r2 = mk();
const A1 = r1[0], B1 = r1[1], C1 = r1[2], A2 = r2[0], B2 = r2[1];
const b: any = new B1(), c: any = new C1();
console.log(Object.getPrototypeOf(B1) === A1, Object.getPrototypeOf(B1.prototype) === A1.prototype, Object.getPrototypeOf(C1) === B1);
console.log(b instanceof B1, b instanceof A1, b instanceof A2, b instanceof B2, c instanceof A1, c instanceof B2);
console.log(b.a(), b.who(), c.who(), b.sum(), b.tag, c.tag);
console.log(B1.s() === B1, B1.t() === B1, C1.t() === C1, B1 !== B2, Object.getPrototypeOf(B2) === A1, Object.getPrototypeOf(B2) === A2);
const m: any = B1.make.call(A1);
console.log(m instanceof A1, m instanceof B1, m.tag, B1.name, C1.name);
console.log(b.constructor === B1, c.constructor === C1, Object.getOwnPropertyNames(B1.prototype).join(","));
// A family one member of which is not admissible (a private element
// on the chain is K5-2's) keeps the shared identity as a whole, and
// answers consistently within an evaluation.
function priv(): any {
  class P { #k = 1; k() { return this.#k; } }
  class Q extends P { q() { return this.k() + 1; } }
  return [P, Q];
}
const p1 = priv();
const q: any = new p1[1]();
console.log(q instanceof p1[0], q instanceof p1[1], q.q());
function named(): any {
  class N { n() { return 1; } }
  class M extends N { m() { return new N().n() + 1; } }
  return [N, M];
}
const n1 = named();
const mm: any = new n1[1]();
console.log(mm instanceof n1[0], mm.m(), new n1[0]() instanceof n1[0]);
