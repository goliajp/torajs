// a constructor reached through a class value runs as written for its
// arguments: a number parameter binds the string it was handed, a
// defaulted one takes its default when the argument is missing, and a
// class-typed one binds the object it was handed as it is, so
// `d1.p === q` holds.
class P {
  x = 1;
}
class D {
  n: any;
  p: any;
  constructor(p: P, n: number = 5) {
    this.p = p;
    this.n = n;
  }
}
class E {
  n: any;
  b: any;
  p: any;
  constructor(p: P, n: number, b: boolean) {
    this.p = p;
    this.n = n;
    this.b = b;
  }
}
const DD: any = D;
const EE: any = E;
const q: any = { x: 9 };
const d1 = new DD(q);
console.log(d1.n, typeof d1.n, d1.p.x, d1.p === q);
const d2 = new DD(q, 3);
console.log(d2.n, typeof d2.n);
const e1 = new EE(q, "8", 0);
console.log(e1.n, typeof e1.n, e1.b, typeof e1.b);
const e2 = new EE(new P(), 8, true);
console.log(e2.n, typeof e2.n, e2.b, e2.p.x);
