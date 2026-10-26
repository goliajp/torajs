// An instance of class L reaches a binding typed with an alias of the
// unrelated class H, and a field typed H of another class, by way of a
// value of any kind. A store through either receiver writes L's own
// field of the same name, which L's method then reads.
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
class L {
  next: D;
  constructor(d: D) {
    this.next = d;
  }
  sum(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      s += this.next.y;
    }
    return s;
  }
}
type HB = H;
class W {
  h: H;
  constructor(h: H) {
    this.h = h;
  }
}
const l = new L(new D(1));
const anyv: any = [l];
const h: HB = anyv[0];
h.next = JSON.parse('{"y": 2.5}');
console.log(l.sum(2), l.next.y);
const l2 = new L(new D(2));
const anyw: any = { h: l2 };
const w: W = anyw;
w.h.next = JSON.parse('{"y": 3.5}');
console.log(l2.sum(2), l2.next.y);
console.log(new W(new H(new C(4))).h.next.x);
