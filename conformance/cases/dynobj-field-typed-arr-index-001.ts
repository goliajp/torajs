// chunk 704 — a typed array stored as a dynobj literal field must be
// elem-kind marked so any-index reads decode raw slots (was undefined).
const o: any = { xs: [1.5, 2.5], bs: [true, false], ss: ["a", "b"], nn: [[1], [2, 3]], is: [10, 20] };
console.log(o.xs[0], o.xs[1]);
console.log(o.bs[0], o.bs[1]);
console.log(o.ss[0], o.ss[1]);
console.log(o.nn[1][1]);
console.log(o.is.length);
for (const v of o.is) console.log(v);
const t: any = o.is;
console.log(t[1]);
console.log(o.is[1]);
