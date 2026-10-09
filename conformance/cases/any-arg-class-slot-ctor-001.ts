// A class reached through a value constructs through its factory's
// boxed entry: an argument that is no instance of the parameter's
// class runs the constructor on the value as it is.
class P {
  x = 1;
  m() {
    return "P" + this.x;
  }
}
class P2 extends P {
  m() {
    return "P2" + this.x;
  }
}
class Q {
  x = 1;
  m() {
    return "Q" + this.x;
  }
}
class K {
  tag: string;
  p: P;
  constructor(p: P) {
    this.p = p;
    this.tag = p.m() + ":" + (p instanceof P);
  }
}
class R {
  s: string;
  constructor(p: P) {
    this.s = "x=" + p.x;
  }
}
class N {
  s: string;
  constructor(p: P | null) {
    this.s = p === null ? "null" : p.m() + ":" + (p instanceof P);
  }
}
type PA = P;
class A {
  s: string;
  constructor(p: PA) {
    this.s = p.m() + ":" + (p instanceof P);
  }
}
const KK: any = K;
const RR: any = R;
const NN: any = N;
const AA: any = A;

console.log(new KK(new Q()).tag, new KK(new P()).tag, new KK(new P2()).tag);
const lit = { x: 5, m() { return "L" + this.x; } };
console.log(new KK(lit).tag);
console.log(new RR(JSON.parse('{"x":7}')).s, new RR(new Q()).s, new RR(3).s);
function err(f: () => any): string {
  try {
    return String(f());
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
console.log(err(() => new KK(3).tag), err(() => new KK(undefined).tag), err(() => new KK().tag));
console.log(err(() => new RR(undefined).s), err(() => new KK(JSON.parse('{"x":1}')).tag));
const q = new Q();
const k = new KK(q);
console.log(k.p === q, k.p instanceof Q, k instanceof K);
const pp = new P();
console.log(new KK(pp).p === pp);
console.log(new NN(null).s, new NN(new Q()).s, new NN(new P()).s);
console.log(new AA(new Q()).s, new AA(new P2()).s);
console.log(Reflect.construct(KK, [new Q()]).tag);
function mk(c: any, v: any) {
  return new c(v);
}
console.log(mk(K, new Q()).tag, mk(K, new P()).tag);
class S2 extends K {}
const SS2: any = S2;
console.log(new SS2(new Q()).tag, new SS2(new Q()) instanceof K);
// a typed construction keeps the typed body
console.log(new K(new P2()).tag, new R(new P()).s);
