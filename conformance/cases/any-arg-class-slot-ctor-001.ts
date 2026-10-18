// A class reached through a value constructs through its factory's
// boxed entry. A parameter the program annotated with a class is a
// class slot like a `let` of that class: an instance of the class (or
// of a subclass) crosses as it is, an object of the class's field types
// that is no class instance becomes a fresh instance, and anything else
// (an instance of another class, a number, undefined, an object owning
// a property under one of the class's method names) throws a
// TypeError before the constructor runs. bun binds the argument as it
// is, so those lines differ from it (the .expected).
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
  p: any;
  constructor(p: P) {
    this.p = p;
    this.tag = p.m() + ":" + (p instanceof P);
  }
}
class KP {
  p: P;
  constructor(p: P) {
    this.p = p;
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

function err(f: () => any): string {
  try {
    return String(f());
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
console.log(err(() => new KK(new Q()).tag), new KK(new P()).tag, new KK(new P2()).tag);
const lit = { x: 5, m() { return "L" + this.x; } };
console.log(err(() => new KK(lit).tag));
console.log(new RR(JSON.parse('{"x":7}')).s, err(() => new RR(new Q()).s), err(() => new RR(3).s));
console.log(err(() => new KK(3).tag), err(() => new KK(undefined).tag), err(() => new KK().tag));
console.log(err(() => new RR(undefined).s), err(() => new KK(JSON.parse('{"x":1}')).tag));
const q = new Q();
console.log(err(() => new KK(q).p === q));
const pp = new P();
console.log(new KK(pp).p === pp);
console.log(new NN(null).s, err(() => new NN(new Q()).s), new NN(new P()).s);
// an alias of the class keeps the value as it is
console.log(new AA(new Q()).s, new AA(new P2()).s);
console.log(err(() => Reflect.construct(KK, [new Q()]).tag));
function mk(c: any, v: any) {
  return new c(v);
}
console.log(err(() => mk(K, new Q()).tag), mk(K, new P()).tag);
class S2 extends K {}
const SS2: any = S2;
console.log(err(() => new SS2(new Q()).tag), new SS2(new P()) instanceof K);
const KKP: any = KP;
console.log(err(() => new KKP(new Q()).p.x), err(() => new KKP(new P2()).p.x));
// a typed construction keeps the typed body
console.log(new K(new P2()).tag, new R(new P()).s);
