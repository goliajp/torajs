// A literal assigned into an `any` binding is the object its methods
// and accessors see as `this`: writes and deletes through `this` land
// on it, and `this` is the very object the binding holds.
let o: any = { x: 1 };
o = {
  get a() {
    this.c = 9;
    delete this.b;
    return 1;
  },
  b: 2,
  c: 3,
};
console.log(o.a, o.c, "b" in o, Object.keys(o).join());

let m: any = { x: 1 };
m = {
  same() {
    return this === m;
  },
  bump() {
    this.n = this.n + 1;
    return this.n;
  },
  n: 0,
};
console.log(m.same(), m.bump(), m.bump(), m.n);

let v: any = { x: 1 };
v = {
  get a() {
    delete this.b;
    return 1;
  },
  b: 2,
  c: 3,
};
console.log(Object.values(v).join());
