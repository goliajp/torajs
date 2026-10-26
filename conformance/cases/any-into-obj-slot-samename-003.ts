// An instance of class L reaches a slot typed as the unrelated class H
// along paths other than an `any` argument: through an array of any
// values, a function returning an `any` value as H, and a binding of
// type H initialized with one. A store through the H-typed receiver
// writes L's own field of the same name, which L's method then reads.
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

function st(h: H, v: any): void {
  h.next = v;
}
function asH(x: any): H {
  return x;
}

const l1 = new L(new D(1));
const xs: any[] = [l1];
st(xs[0], { y: "a" });
console.log(l1.sum(2), l1.next.y);

const l2 = new L(new D(2));
st(asH(l2), { y: "b" });
console.log(l2.sum(2), l2.next.y);

const l3 = new L(new D(3));
const box: any = l3;
const h3: H = box;
h3.next = JSON.parse('{"y": 2.5}');
console.log(l3.sum(2), l3.next.y);
console.log(new H(new C(4)).next.x);
