// A function an object literal of an interface whose method returns an
// any value holds in that member is also handed to a parameter typed
// with its function type. The callee reads its declared return, so the
// function keeps it; the interface's member returns any, and a call
// through it boxes the function's answer.
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
const twice = (h: () => number) => h() + h();
function e(): number {
  return 8;
}
const o: J = { m: e, n: 4 };
console.log(call(o), twice(e), call(new S(raw)), call(new T(raw)), call(new U(raw)));
