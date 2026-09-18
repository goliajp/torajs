// §10.2.2 step 13 through a REAL class parent on the runtime-value lane:
// the parent's receiver-polymorphic constructor twin answers what its body
// returned, directly, through a ctor-less middle class, and when the class
// is reached as a runtime value. A parent that returns nothing is unchanged.
// real-class parent with return override, capturing derived class
class Base4 { constructor(o: any) { return o; } }
function mk4(tagv: string): any { return class extends Base4 { t = tagv; constructor(o: any) { super(o); } }; }
const D4: any = mk4("d");
const o4: any = { q: 1 };
const d4: any = new D4(o4);
console.log(d4 === o4, d4.t, d4.q);
class Base { constructor(o: any) { return o; } }
class Mid extends Base { m = 1; }
function mk(tagv: string): any { return class extends Mid { t = tagv; constructor(o: any) { super(o); } }; }
const D: any = mk("d");
const o: any = { q: 1 };
const d: any = new D(o);
console.log(d === o, d.t, d.q, d.m);
const viaValue: any = Base;
class K extends viaValue { k = 2; }
const o2: any = { z: 9 };
const k: any = new K(o2);
console.log(k === o2, k.k, k.z);
class Plain { p: number; constructor(n: number) { this.p = n; } }
function mkp(v: number): any { return class extends Plain { w = v; }; }
const W: any = mkp(5);
const w: any = new W(3);
console.log(w.p, w.w, w instanceof Plain);
