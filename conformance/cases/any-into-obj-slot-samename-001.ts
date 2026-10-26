// Two unrelated classes declare a field of the same name, each with its
// own object type. A store of an `any` value into one of them widens the
// field under its name, so the other one's field holds its value as it
// is too: the answers stay what bun prints, and the typed lane the
// second class loses is what the typed census counts here.
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

const raw: any = JSON.parse('{"x": "s"}');
const h = new H(new C(1));
st(h, raw);
console.log(h.next === raw, h.next.x);

const l = new L(new D(2));
console.log(l.sum(3), l.next.y);
l.next = new D(5);
console.log(l.sum(2), l.next instanceof D);
