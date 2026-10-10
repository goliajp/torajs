// A constructor that reads `arguments` and declares class-, array- or
// function-typed parameters, reached by a spread argument list: the
// implicit constructor of a derived class forwards `...arguments`, and
// `new C(...xs)` spreads at the `new` site. Each declared parameter
// still binds its own value, and `arguments` counts the list.
class Pt {
  x = 3;
}
class P {
  s: number;
  v: number;
  constructor(p: Pt, f: (n: number) => number, xs: number[]) {
    this.s = arguments.length;
    this.v = p.x + f(1) + xs.length;
  }
}
class Q extends P {}
const r = new Q(new Pt(), (n: number) => n + 1, [1, 2]);
console.log(r.s, r.v);
const q = new P(...([new Pt(), (n: number) => n * 10, [4]] as [Pt, (n: number) => number, number[]]));
console.log(q.s, q.v);

class A {
  s: number;
  v: number;
  constructor(xs: number[]) {
    this.s = arguments.length;
    this.v = xs.length;
  }
}
class B extends A {}
const b = new B([1, 2]);
console.log(b.s, b.v);
const nested: number[][] = [[4, 5, 6]];
const a = new A(...nested);
console.log(a.s, a.v);

class C {
  s: number;
  v: number;
  constructor(p: Pt) {
    this.s = arguments.length;
    this.v = p.x;
  }
}
class D extends C {}
const d = new D(new Pt());
console.log(d.s, d.v);
const ps: Pt[] = [new Pt()];
const c = new C(...ps);
console.log(c.s, c.v);
