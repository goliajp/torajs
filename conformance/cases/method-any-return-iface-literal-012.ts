// A const binding of a closure an object literal of an interface whose
// method returns an any value holds in that member is also called
// directly. The call reads the closure's own return, so the closure
// keeps it; the interface's member returns any, and a call through it
// boxes the closure's answer.
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
const k = () => 9;
const o: J = { m: k, n: 1 };
const r = k();
console.log(call(o), r * 2, typeof r, call(new S(raw)), call(new T(raw)), call(new U(raw)));
