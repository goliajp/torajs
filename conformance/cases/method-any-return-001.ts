// A method declared to return a number hands back what an any value
// holds as it is (§10.2.1.12): a fraction stays a fraction and a string
// stays a string, whether the any value is a field, a parameter's
// member, a getter's or a static method's answer, or a generic class's.
const raw: any = JSON.parse('{"x": 1.5, "y": "s"}');

class K {
  c: any;
  constructor(v: any) {
    this.c = v;
  }
  frac(): number {
    return this.c.x;
  }
  str(): number {
    return this.c.y;
  }
  get g(): number {
    return this.c.x;
  }
  static s(v: any): number {
    return v.y;
  }
}
const k = new K(raw);
console.log(String(k.frac()), String(k.str()), String(k.g), String(K.s(raw)));
const n: number = k.frac() + 1;
console.log(n);

// a field typed by a declared object type that is later stored an any
// value: the method reading through it returns what was stored
type T = { x: number };
type W = { t: T };
class C {
  m(w: W): number {
    return w.t.x;
  }
}
const v: any = JSON.parse('{"x":1.5}');
console.log(String(new C().m({ t: v })));
console.log(String(new C().m({ t: { x: 1 } })));
const w: W = { t: { x: 2 } };
w.t = v;
console.log(String(w.t.x));

class Box<T> {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  get(): number {
    return this.v.y;
  }
}
console.log(String(new Box<number>(raw).get()));
