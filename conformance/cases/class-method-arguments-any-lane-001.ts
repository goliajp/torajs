// A class method that reads `arguments` and is called directly with
// one count answers that count on the direct lane at no cost. A call
// through `any` (or a re-bound method value) may pass any other
// count, and must see exactly the arguments it passed (§10.4.4.6).
// An omitted optional parameter is no argument either, whatever its
// type (§10.2.11: it binds undefined, the count stays as written).

class F {
  v = 9;
  n(a: number) {
    return arguments.length + ":" + arguments[1];
  }
  o(a?: number) {
    return arguments.length + ":" + a + ":" + arguments[1];
  }
  b(x: boolean) {
    return arguments.length + ":" + x + ":" + arguments[1];
  }
  t(a: number) {
    return this.v + ":" + arguments.length + ":" + arguments[1];
  }
}

const f = new F();
const r: any = f;
console.log(f.n(1), r.n(1, 2), r.n(), r.n(1));
console.log(f.o(1), r.o(1, 2), r.o());
console.log(f.b(true), r.b(false, 7), r.b());
console.log(f.t(1), r.t(1, 2));
console.log(F.prototype.n.call(f, 1, 2, 3), F.prototype.n.apply(f, [4, 5]));
const g = f.n;
console.log((g as any).call(f, 4, 5, 6));
console.log(F.prototype.n.length, F.prototype.b.length);

class G extends F {
  w() {
    return super.n(5);
  }
}
const gg = new G();
console.log(gg.w(), (gg as any).n(3, 4));

class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  m(a: T) {
    return arguments.length + ":" + arguments[1];
  }
}
const bx = new Box<number>(1);
console.log(bx.m(2), (bx as any).m(2, 3));

class H {
  n(a: number) {
    return arguments.length + ":" + arguments[2];
  }
}
const xs = [1, 2, 3];
const h = new H();
console.log(h.n(1), h.n(...xs), (h as any).n(4, 5, 6));

class D {
  d(a: number, b: number = 5) {
    return arguments.length + ":" + b;
  }
  u(a: number, b?: string) {
    return arguments.length + ":" + b + ":" + arguments[2];
  }
  l(a: number, b?: string) {
    return arguments.length;
  }
}
const d = new D();
const rd: any = d;
console.log(d.d(1), d.d(1, 2), rd.d(1, 2, 3));
console.log(d.u(1, "y"), rd.u(1, "x"), rd.u(1, "x", 9), rd.u(), rd.u(4));
console.log(rd.l(), rd.l(1), rd.l(1, "x", 3));
