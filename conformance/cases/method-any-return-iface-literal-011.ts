// A binding annotated with a function type holds the closure an object
// literal of an interface whose method returns an any value holds in
// that member. The binding is called through its annotation, so the
// closure keeps its return; the interface's member returns any, and a
// call through it boxes the closure's answer.
const raw: any = JSON.parse('{"x":1.5,"s":"ab","o":{"k":3}}');
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
class T implements J {
  v: any;
  n = 2;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.s;
  }
}
class U implements J {
  v: any;
  n = 3;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.o;
  }
}
function call(o: J): number {
  return o.m() + o.n;
}
const f: () => number = () => 4;
const o: J = { m: f, n: 2 };
const r = f();
console.log(call(o), r + 1, typeof r, call(new S(raw)), call(new T(raw)), call(new U(raw)));
