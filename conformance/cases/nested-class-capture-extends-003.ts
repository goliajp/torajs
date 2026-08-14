// RFC 20260814 blade 5 / 405-01 — extends with a STATIC-CARRYING
// parent, and 3-deep chains. The class-side link is
// `Object.setPrototypeOf(D, P)` riding the function value's user
// [[Prototype]] chain; a ctor-less middle class is skipped by the
// super forward (its synthesized rest-param forwarder is the shape
// the promotion ABI bar refuses).

// parent with statics: inherited static call + instance method
{
  let a = 1;
  class P0 {
    static s() {
      return a + 1;
    }
    m() {
      return 10;
    }
  }
  class D0 extends P0 {}
  const d = new D0();
  console.log(D0.s(), d.m());
  console.log(d instanceof D0, d instanceof P0);
}

// per-call identity: each enclosing call mints its own pair
function mk(n: any) {
  class P1 {
    static base() {
      return n;
    }
  }
  class D1 extends P1 {
    static extra() {
      return P1.base() + 1;
    }
  }
  return D1;
}
const A = mk(5);
const B = mk(7);
console.log(A.base(), B.base(), A.extra(), B.extra());

// 3-deep chain, static override in the middle
{
  let a = 2;
  class P2 {
    static s() {
      return a;
    }
    static t() {
      return 100;
    }
  }
  class C2 extends P2 {
    static t() {
      return 200;
    }
  }
  class D2 extends C2 {}
  console.log(D2.s(), D2.t(), C2.s());
}

// class expression extending a static-carrying capturing parent
{
  let a = 3;
  class P3 {
    static mk() {
      return a * 10;
    }
  }
  const D3 = class extends P3 {
    m() {
      return a;
    }
  };
  console.log((D3 as any).mk(), new D3().m());
}

// static-free 3-deep chain (the pre-existing middle-class hole:
// its synthesized forwarder used to refuse receiver promotion)
{
  let a = 2;
  class P4 {
    m() {
      return a;
    }
  }
  class C4 extends P4 {}
  class D4 extends C4 {}
  console.log(new D4().m());
}

// explicit middle ctor: super forwards INTO it, not past it
{
  let a = 4;
  class P5 {
    v: any;
    constructor(x: any) {
      this.v = x - a;
    }
  }
  class C5 extends P5 {
    constructor(x: any) {
      super(x + a + a);
    }
  }
  class D5 extends C5 {}
  console.log(new D5(10).v);
}
