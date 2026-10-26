// A binding annotated with a function type holds the closure an object
// literal of an interface whose method returns an any value holds in
// that member. The binding is called through its annotation, so the
// closure keeps its return, and so does the interface's family.
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
const f: () => number = () => 4;
const o: J = { m: f, n: 2 };
const r = f();
console.log(call(o), r + 1, typeof r, call(new S(raw)));
