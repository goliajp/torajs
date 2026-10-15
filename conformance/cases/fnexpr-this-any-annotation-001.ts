// `this: any` on a function expression types the body's `this` as
// `any`, whatever position the expression sits in: a field of an
// object literal with no annotation, a value read back out of one
// through Object.values / Object.entries.
const o = {
  tag: 0,
  set: function (this: any, v: number) {
    this.tag = v;
    return this.tag;
  },
};
console.log(o.set(5), o.tag);

const p = { f: function (this: any) { return this.k } };
const fs = Object.values(p);
console.log((fs[0] as any).call({ k: 6 }));

const q = { F: function (this: any) { this.tag = 3 } };
const es = Object.entries(q);
console.log(new (es[0][1] as any)().tag);

// another annotation keeps the position-derived type
const r = { n: 2, get: function (this: { n: number }) { return this.n } };
console.log(r.get());
