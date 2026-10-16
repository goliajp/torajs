// §10.1.10 [[Delete]] of an own accessor; §10.1.8.1 OrdinaryGet then
// continues on the prototype the program set (§20.1.2.23
// Object.setPrototypeOf), and a getter that answered an object answers
// what the prototype holds.
const p = { n: 42 };
const o = {
  get n() {
    return 7;
  },
  get m() {
    delete (this as any).n;
    return this.n;
  },
};
Object.setPrototypeOf(o, p);
console.log(o.n, o.m, o.n);
const q = {
  get box() {
    return { v: 1 };
  },
  k: 2,
};
console.log(q.box.v);
delete (q as any).box;
console.log(q.box, "box" in q, q.k);
const r = {
  get box() {
    return { v: 3 };
  },
  get m() {
    delete (this as any).box;
    return this.box;
  },
};
console.log(r.box.v);
console.log(r.m, r.box);
