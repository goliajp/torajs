// A function expression or declaration written inside a class body
// has its own `this` (§10.2.1.2): `new` hands it the fresh instance
// (§10.2.2), a plain call hands it undefined, `.call(o)` hands it `o`.
// None of them may read the enclosing method's receiver, which is what
// an unclaimed `this` used to capture.
class C {
  z = 5;
  m(): void {
    const F = function (this: any, a: number) { this.tag = a; };
    function G(this: any) { this.tag = 2; }
    const f = function (this: any) { return this === undefined; };
    function g(this: any) { return this === undefined; }
    const h = function (this: any) { return this.z; };
    function k(this: any) { return this.z; }
    console.log("m new", new (F as any)(1).tag, new (G as any)().tag);
    console.log("m call", f(), g());
    console.log("m .call", h.call({ z: 7 }), k.call({ z: 8 }));
  }
  static s(): void {
    const F = function (this: any) { this.tag = 6; };
    function G(this: any) { this.tag = 16; }
    const f = function (this: any) { return this === undefined; };
    const h = function (this: any) { return this.z; };
    console.log("s new", new (F as any)().tag, new (G as any)().tag);
    console.log("s call", f(), h.call({ z: 9 }));
  }
  constructor() {
    const F = function (this: any) { this.tag = 7; };
    function G(this: any) { this.tag = 17; }
    const f = function (this: any) { return this === undefined; };
    const h = function (this: any) { return this.z; };
    console.log("ctor new", new (F as any)().tag, new (G as any)().tag);
    console.log("ctor call", f(), h.call({ z: 10 }));
  }
}
const c = new C();
c.m();
C.s();
console.log("field", c.z, (c as any).tag);
