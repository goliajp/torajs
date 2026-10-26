// Why a field widened by name is widened in an unrelated class too: an
// instance of L reaches the parameter `h: H` as an any value, and the
// store through `h` writes L's own `next` slot, which L's method then
// reads. Without the widening L would read the stored object at D's
// offsets.
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

const h = new H(new C(1));
console.log(h.next.x);
const lx: any = new L(new D(3));
console.log(lx.sum(2), lx.next.y);
st(lx, { y: "t" });
console.log(lx.sum(2), lx.next.y);
st(lx, JSON.parse('{"y": 1.5}'));
console.log(lx.sum(2), lx.next.y);
const l: L = lx;
console.log(l.sum(1), l.next.y);
