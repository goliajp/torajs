// A method that returns an any value, reached through an object type
// literal written in place: one of the shape of a declared interface
// the class implements, and one no declaration spells. Every position
// that spells such a literal (parameter, binding, return, element,
// nested member) still takes the class instance and calls the method,
// which hands the value back as it is.
const raw: any = JSON.parse('{"x": 1.5}');

interface I {
  m(): number;
}
class P implements I {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function f(o: { m(): number }): number {
  return o.m();
}
console.log(f(new P(raw)));

function g(o: { k(): number }) {
  return o.k();
}
class Q {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  k(): number {
    return this.v.x;
  }
}
console.log(g(new Q(raw)));

interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 3;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class R implements J {
  n = 4;
  m(): number {
    return 0.5;
  }
}
function h(o: { m(): number; n: number }): number {
  return o.m() + o.n;
}
console.log(h(new S(raw)), h(new R()));
let a: { m(): number; n: number } = new S(raw);
console.log(a.m());
a = new R();
console.log(a.m());
function mk(v: any): { m(): number; n: number } {
  return new S(v);
}
console.log(mk(raw).m());
const xs: { m(): number; n: number }[] = [new S(raw), new R()];
for (const x of xs) console.log(x.m(), x.n);
const lit: { m(): number; n: number } = { m: () => 6.5, n: 1 };
console.log(h(lit), lit.m());
function wrap(o: { inner: { m(): number; n: number } }): number {
  return o.inner.m();
}
console.log(wrap({ inner: new S(raw) }));
