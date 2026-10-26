// An instance of class L reaches a slot typed with the unrelated class
// H inside another type: an object type's member, a function's result,
// a map's, a dictionary's and a promise's value, and an object type's
// method. Each hands on a value of any kind, and a store through the
// H-typed receiver writes L's own field of the same name.
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
const l1 = new L(new D(1));
const any1: any = l1;
const pack = { h: any1 };
const b: { h: H } = pack;
b.h.next = JSON.parse('{"y": 2.5}');
console.log(l1.sum(2), l1.next.y);

const l2 = new L(new D(2));
const any2: any = l2;
const mk = () => any2;
const g: () => H = mk;
const h2: H = g();
h2.next = JSON.parse('{"y": 3.5}');
console.log(l2.sum(2), l2.next.y);

const l3 = new L(new D(3));
const m = new Map<string, any>();
m.set("a", l3);
const mh: Map<string, H> = m;
const h3: H = mh.get("a")!;
h3.next = JSON.parse('{"y": 4.5}');
console.log(l3.sum(2), l3.next.y);

const l4 = new L(new D(4));
const any4: any = l4;
const d: Record<string, any> = { a: any4 };
const dh: Record<string, H> = d;
const h4: H = dh["a"];
h4.next = JSON.parse('{"y": 5.5}');
console.log(l4.sum(2), l4.next.y);

const l5 = new L(new D(5));
const any5: any = l5;
const box: { get(): any } = { get: () => any5 };
const viaFn: { get(): H } = box;
const h5: H = viaFn.get();
h5.next = JSON.parse('{"y": 6.5}');
console.log(l5.sum(2), l5.next.y);

const l6 = new L(new D(6));
const any6: any = l6;
const p: Promise<any> = Promise.resolve(any6);
const ph: Promise<H> = p;
ph.then((h: H) => {
  h.next = JSON.parse('{"y": 7.5}');
  console.log(l6.sum(2), l6.next.y);
});
console.log(new H(new C(4)).next.x);
