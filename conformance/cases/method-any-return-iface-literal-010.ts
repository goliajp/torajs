// A function an object literal of an interface whose method returns an
// any value holds in that member is also held by a literal of another
// interface, whose member keeps its declared return. The function keeps
// its own; the first interface's member returns any, and a call through
// it boxes the function's answer.
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
interface K2 {
  m(): number;
}
function callK(o: K2): number {
  return o.m() * 3;
}
function g(): number {
  return 5;
}
const o: J = { m: g, n: 1 };
const o2: K2 = { m: g };
console.log(call(o), callK(o2), call(new S(raw)), call(new T(raw)), call(new U(raw)));
