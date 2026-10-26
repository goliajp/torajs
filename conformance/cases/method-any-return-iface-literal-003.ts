// An object literal of the type of an interface whose method returns an
// any value: the closure it holds in that member is called through the
// interface's signature, so it returns its value as it is too, whether
// the object reaches a parameter typed by the interface, one typed by
// an object type literal of its shape, or a binding.
const raw: any = JSON.parse('{"x":1.5}');

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
function h3(o: { m(): number; n: number }) {
  return o.m() + o.n;
}
function h5(o: { m(): number; n: number }) {
  const a = o.m();
  return a * 2;
}
function hj(o: J) {
  return o.m() - o.n;
}
console.log(h3({ m: () => 4, n: 3 }), h3(new S(raw)));
console.log(h5({ m: () => 2.5, n: 0 }), hj({ m: () => 9, n: 1 }));
console.log(hj({ m: () => 0.25, n: 1 }), hj(new S(raw)));
const lit: J = {
  m() {
    return 7;
  },
  n: 2,
};
console.log(h3(lit), hj(lit), lit.m());
const xs: J[] = [{ m: () => 1, n: 1 }, new S(raw)];
for (const x of xs) console.log(x.m() + x.n);
