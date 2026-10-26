// A field widened to any by name in one class, and an unrelated class
// with a field of the same name whose instances only go to slots typed
// with it: arrays of it, nested arrays, object literals laid out from
// it in a binding, an array and a return. The second class keeps its
// type, and every read of it answers the value stored.
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
class K2 {
  next: D;
  constructor(d: D) {
    this.next = d;
  }
}
function st(h: H, v: any): void {
  h.next = v;
}
const raw: any = JSON.parse('{"x": "s"}');
const h = new H(new C(1));
st(h, raw);
console.log(h.next === raw, h.next.x);
const xs: L[] = [new L(new D(2)), new L(new D(3))];
console.log(xs[0].sum(2), xs[1].next.y);
const ks: K2[] = [{ next: new D(4) }];
const k1: K2 = { next: new D(5) };
function mk(): K2 {
  return { next: new D(6) };
}
console.log(ks[0].next.y, k1.next.y, mk().next.y);
const grid: L[][] = [[new L(new D(7))]];
console.log(grid[0][0].sum(3));
