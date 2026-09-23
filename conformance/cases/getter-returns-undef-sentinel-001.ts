// An accessor read is an `Expr::Member`, so the "may this hold the
// sentinel" question took the field arm — which asks whether some
// WRITE filled a field of that name. A getter name is never written,
// so a getter handing back an out-of-range read reported `typeof`
// "number" for an `undefined` the plain field read got right.
const zs: number[] = [];

class C {
  a: number = 0;
  constructor() {
    this.a = zs[9];
  }
  get g(): number {
    return this.a;
  }
  get direct(): number {
    return zs[9];
  }
  get lit(): number {
    return 7;
  }
}
const c = new C();
console.log("field", typeof c.a, c.a, c.a === undefined);
console.log("getter", typeof c.g, c.g, c.g === undefined);
console.log("direct", typeof c.direct, c.direct, c.direct === undefined);
console.log("live", typeof c.lit, c.lit, c.lit === undefined);

class D {
  static get s(): number {
    return zs[9];
  }
}
console.log("static", typeof D.s, D.s, D.s === undefined);

// A getter over a live value keeps the plain number lane.
class E {
  v: number = 3;
  get twice(): number {
    return this.v * 2;
  }
}
console.log("arith", typeof new E().twice, new E().twice);
