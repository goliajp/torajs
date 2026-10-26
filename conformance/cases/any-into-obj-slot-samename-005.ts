// An instance of class L reaches a binding, a return and a cast typed
// with an interface whose member is typed as the unrelated class H, by
// way of a value of any kind. A store through that member writes L's
// own field of the same name, which L's method then reads: an
// annotation names H through the interface as well as by its name.
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
interface Box {
  h: H;
}
const l = new L(new D(1));
const anyv: any = { h: l };
const b: Box = anyv;
b.h.next = JSON.parse('{"y": 2.5}');
console.log(l.sum(2), l.next.y);
const l2 = new L(new D(2));
const anyw: any = { h: l2 };
function boxed(): Box {
  return anyw;
}
boxed().h.next = JSON.parse('{"y": 3.5}');
const l3 = new L(new D(3));
const anyx: any = { h: l3 };
(anyx as Box).h.next = JSON.parse('{"y": 4.5}');
console.log(l2.sum(2), l2.next.y, l3.sum(2), l3.next.y);
console.log(new H(new C(4)).next.x);
