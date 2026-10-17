// §10.1.10 [[Delete]] through an `any` receiver removes an object
// literal's accessor member: [[Get]] / [[HasProperty]] then find no own
// property, and a store creates an ordinary data property at the end of
// the key order (§10.1.9.2 → CreateDataProperty) instead of running the
// old setter or refusing as a getter-only accessor. The typed read of
// the same member agrees with the `any` read.
const o = { get n() { return 7 }, set n(v: number) { console.log("setter", v) }, k: 1 };
const a: any = o;
console.log(a.n, o.n);
delete a.n;
console.log(a.n, o.n);
console.log("n" in a);
console.log(a["n"]);
a.n = 5;
console.log(a.n, o.n);
console.log(Object.keys(a).join(","));
const p = { get z() { return { v: 1 } } };
const b: any = p;
delete b.z;
console.log(b.z, p.z);
const q = { get w() { return 3 } };
const c: any = q;
delete c.w;
c.w = 9;
console.log(c.w, q.w, JSON.stringify(c));
