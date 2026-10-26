// A field widened to any by name in one class, and an unrelated class
// with a field of the same name whose object literals are written where
// a union with it is annotated. Such a literal is not read as laid out
// from the class, and every read of it still answers the value stored.
class C {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
class D {
  y: number;
  constructor(y: number) {
    this.y = y;
  }
}
class H {
  next: C;
  constructor(c: C) {
    this.next = c;
  }
}
class K2 {
  next: D;
  constructor(d: D) {
    this.next = d;
  }
}
class M {
  next: D;
  tag: string;
  constructor(d: D) {
    this.next = d;
    this.tag = "m";
  }
}
function st(h: H, v: any): void {
  h.next = v;
}
const raw: any = JSON.parse('{"x": "s"}');
const h = new H(new C(1));
st(h, raw);
console.log(h.next === raw, h.next.x);
const u: K2 | M = { next: new D(2) };
const n: K2 | null = { next: new D(3) };
console.log(u.next.y, n === null ? 0 : n.next.y);
function mk(): K2 | M {
  return { next: new D(5) };
}
console.log(mk().next.y, new K2(new D(6)).next.y, new M(new D(7)).next.y);
