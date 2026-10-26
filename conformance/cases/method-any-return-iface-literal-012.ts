// A const binding of a closure an object literal of an interface whose
// method returns an any value holds in that member is also called
// directly. The call reads the closure's own return, so the closure
// keeps it, and so does the interface's family.
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
const k = () => 9;
const o: J = { m: k, n: 1 };
const r = k();
console.log(call(o), r * 2, typeof r, call(new S(raw)));
