// Two unrelated classes declare a field of the same name, each with its
// own object type. A store of an `any` value into one of them widens
// that class's field; no value of any kind reaches a slot typed with
// the first class and no instance of the second leaves its own type,
// so the second class's field keeps its type, and its typed lane is
// what the typed census counts here.
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
