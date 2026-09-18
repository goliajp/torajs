// K5-4 — a per-evaluation class reached as a runtime-value parent
// (`class extends Base`): `super(…)` runs the evaluation's constructor
// through its template's twin, and identity stays per evaluation.
function base(): any {
  class P {
    v = 1;
    constructor(public tag: string = "t") {}
    hi() { return "p" }
    static make() { return new this("made") }
    static sname() { return "P" }
  }
  return P
}
function mk(Base: any): any {
  return class extends Base {
    w: number;
    constructor(...args: any[]) { super(...args); this.w = 2 }
    hi() { return "d>" + super.hi() }
    static sname() { return "D>" + super.sname() }
  }
}
const P1 = base(), P2 = base(), D1 = mk(P1), D2 = mk(P2), E1 = mk(D1);
const d: any = new D1("x"), e: any = new E1();
console.log(d.v, d.w, d.tag, d.hi(), e.hi(), e.tag);
console.log(d instanceof P1, d instanceof P2, e instanceof D1, e instanceof P1, e instanceof D2);
console.log(Object.getPrototypeOf(D1) === P1, Object.getPrototypeOf(E1) === D1, D1 !== D2, P1 !== P2);
console.log(D1.sname(), E1.sname(), D1.make() instanceof D1, D1.make().tag, P1.make() instanceof P1);
console.log(new P1() instanceof P1, new P1() instanceof P2, typeof P1, P1.name);
