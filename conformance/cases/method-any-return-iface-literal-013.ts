// A function an object literal of an interface whose method returns an
// any value holds in that member is also handed to a parameter typed
// with its function type. The callee reads its declared return, so the
// function keeps it, and so does the interface's family.
const raw: any = JSON.parse('{"x":2}');
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
const twice = (h: () => number) => h() + h();
function e(): number {
  return 8;
}
const o: J = { m: e, n: 4 };
console.log(call(o), twice(e), call(new S(raw)));
