// §10.2.2 step 13 on the runtime-value lane: an object the parent
// constructor hands back becomes the derived constructor's `this` —
// fields land on it, arrows see it, a bare `return;` answers it, and a
// written `return <expr>` takes step 13 as the derived class it is.
class K extends (function (this: any, x: any) { return x; } as any) {
  tag = "k";
  constructor(o: any) { super(o); }
}
const target: any = { id: 1 };
const r: any = new K(target);
console.log(r === target, r.tag, r.id);

// a primitive answer is ignored; `this` sites after super see the override
function mkv(v: any): any {
  return class extends (function (this: any) { this.base = 1; return v; } as any) {
    own: number;
    constructor() { super(); this.own = 2; }
    who(): string { return "m"; }
  };
}
const V1: any = mkv(7);
const v1: any = new V1();
console.log(v1.base, v1.own, v1 instanceof V1, v1.who());
const obj: any = { z: 9 };
const V2: any = mkv(obj);
const v2: any = new V2();
console.log(v2 === obj, v2.z, v2.own, v2.base, v2 instanceof V2);

const P: any = function (this: any, x: any) { this.b = 1; return x; };
class A extends P { constructor(o: any, mode: number) { super(o); if (mode === 0) return; if (mode === 1) return { own: 1 } as any; if (mode === 2) return 5 as any; (this as any).fell = true; } }
const t: any = { id: 7 };
const a0: any = new A(t, 0); console.log(a0 === t);
const a1: any = new A(t, 1); console.log(a1 === t, a1.own);
try { new A(t, 2); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError); }
const a3: any = new A(t, 3); console.log(a3 === t, a3.fell);
const a4: any = new A(undefined, 3); console.log(a4.b, a4.fell, a4 instanceof A);

// an arrow captures the adopted this; a nested function keeps its own
class B extends P { get: any; constructor(o: any) { super(o); this.get = () => this; (this as any).f = function (this: any) { return this; }; } }
const tb: any = {};
const b: any = new B(tb);
console.log(b === tb, b.get() === tb, b.f.call(tb) === tb, b.f.call(t) === t);

// two lane classes chained: the grandparent's answer reaches the grandchild
function mkc(o: any): any { class G extends P { g = 1; } class H extends G { h = 2; } return new H(o); }
const th: any = { q: 1 };
const h: any = mkc(th);
console.log(h === th, h.g, h.h, h.q);
