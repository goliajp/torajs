// An object literal of the type of an interface whose method returns an
// any value holds in that member a method read off another object, and
// one bound: neither can be followed to its declaration, so the
// interface's method keeps its declared return, and every function the
// literals hold answers its own value through it.
const raw: any = JSON.parse('{"x":3}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function call(o: J): number {
  return o.m() + o.n;
}
function h3(o: { m(): number; n: number }) {
  const a = o.m();
  return a * 2;
}
const hold = { meth: () => 2.5 };
console.log(h3({ m: hold.meth, n: 3 }), h3(new S(raw)));
const t = new S({ x: 4 });
const p: J = { m: t.m.bind(t), n: 5 };
console.log(h3(p), call(new S(raw)), t.m());
