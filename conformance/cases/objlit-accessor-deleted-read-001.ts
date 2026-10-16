// §10.1.10 [[Delete]] removes an accessor member like any other own
// property; a later [[Get]] (§10.1.8.1) finds no own property and
// answers the prototype's value, here `undefined`, instead of running
// the deleted getter.
const o = {
  get n() {
    return 7;
  },
  get m() {
    delete (this as any).n;
    return (this as any).n;
  },
  k: 1,
};
console.log(o.m, o.n);
const p = {
  get n() {
    return 3;
  },
  f() {
    delete (this as any).n;
    return this.n;
  },
};
console.log(p.n, p.f(), p.n, "n" in p);
const t = {
  get s() {
    return "s";
  },
  get m() {
    delete (this as any).s;
    return this.s;
  },
};
console.log(t.s, t.m, t.s);
const q = {
  get n() {
    return 3;
  },
  k: 0,
};
delete (q as any).n;
console.log(q.n, "n" in q);
const r = {
  get n() {
    return 4;
  },
};
console.log(r.n + 1);
