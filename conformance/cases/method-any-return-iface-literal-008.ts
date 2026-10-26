// Two interfaces of one shape under two names, one of them filled by a
// class whose method returns an any value, and object literals of
// either handed to either. Both are retyped together, so every call
// answers the value the method or the literal's closure returns.
const raw: any = JSON.parse('{"x":1.5,"y":0.25}');
interface J {
  m(): number;
  n: number;
}
interface K {
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
function cj(o: J): number {
  return o.m() + o.n;
}
function ck(o: K): number {
  return o.m() * o.n;
}
const a: J = { m: () => 2, n: 3 };
const b: K = { m: () => 4, n: 5 };
console.log(cj(a), ck(b), cj(b), ck(a), cj(new S(raw)), ck(new S(raw)));
// an interface whose member is typed any, and a second interface of
// the same member names a literal fits as well: that literal's closure
// keeps its own return, and the first interface's method keeps its
interface KD {
  d: any;
  k(): number;
}
interface KN {
  d: number;
  k(): number;
}
class TD implements KD {
  d: any = 1;
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  k(): number {
    return this.v.y;
  }
}
function fd(o: KD) {
  return o.k();
}
function fn(o: KN) {
  return o.k() + o.d;
}
console.log(fd(new TD(raw)), fd({ d: { a: 2 }, k: () => 3 }));
console.log(fn({ d: 5, k: () => 4 }));
const lit = { d: 6, k: () => 0.5 };
console.log(fn(lit), lit.k());
