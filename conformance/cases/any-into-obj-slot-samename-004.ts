// An instance of class L reaches a parameter typed as the unrelated
// class H with no `any` value written at the call: a callback's
// parameter handed the elements of an array of any values, and a
// method called through an any receiver. A store through the
// parameter writes L's own field of the same name.
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
  get(): number {
    return this.next.y;
  }
}
class M {
  put(h: H, v: any): void {
    h.next = v;
  }
}

const raw: any = JSON.parse('{"y": "s"}');
const l1 = new L(new D(1));
const ls: any[] = [l1];
ls.forEach((h: H) => {
  h.next = raw;
});
console.log(l1.get(), typeof l1.next);

const l2 = new L(new D(2));
const m: any = new M();
m.put(l2, { y: 7.5 });
console.log(l2.get() + 1);
console.log(new H(new C(3)).next.x);
