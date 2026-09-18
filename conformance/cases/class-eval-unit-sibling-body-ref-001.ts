// A class named inside a SIBLING class's body is not a unit candidate:
// the sibling's body is hoisted out of the container, so from there
// `new D()` would build through the registered template while the
// container's `D` is this evaluation's class object — and the instance
// would answer false to `instanceof D`.
function mk(): any {
  class D { v = 1; }
  class E { make() { return new D(); } static isD(o: any) { return o instanceof D; } }
  return [D, E];
}
const r1 = mk(), r2 = mk();
const D1 = r1[0], E1 = r1[1], E2 = r2[1];
const d: any = new E1().make();
console.log(d instanceof D1, d.v, E1.isD(d), E1.isD(new D1()), E1 !== E2);
function later(): any {
  class E { make() { return new D(); } }
  class D { v = 2; }
  return [D, E];
}
const l = later();
const d2: any = new l[1]().make();
console.log(d2 instanceof l[0], d2.v);
