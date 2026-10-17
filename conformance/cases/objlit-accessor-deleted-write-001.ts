// §10.1.9.2 [[Set]] through a typed receiver after a delete through an
// `any` alias removed an object literal's accessor member: no own
// accessor is left, so the store creates an ordinary data property
// instead of running the old setter, and the typed and `any` reads
// then answer the stored value. An accessor no delete removed still
// runs its setter, a frozen object's accessor refuses the delete and
// keeps its setter, and a string member takes the same store.
const o = { get n(): number { return 7 }, set n(v: number) { console.log("setter", v) }, k: 1 };
o.n = 1;
console.log(o.n);
const oa: any = o;
delete oa.n;
o.n = 5;
console.log(o.n, oa.n);
console.log(Object.keys(o).join(","), "n" in o);
o.n = 6;
console.log(o.n, (o.n = 9), o.n);
const p = { get n(): number { return 1 }, set n(v: number) { console.log("p setter", v) } };
p.n = 2;
console.log(p.n);
const f = { get n(): number { return 3 }, set n(v: number) { console.log("f setter", v) } };
Object.freeze(f);
console.log(Reflect.deleteProperty(f, "n"));
f.n = 4;
console.log(f.n);
const s = { get t(): string { return "a" }, set t(v: string) { console.log("s setter", v) } };
const sa: any = s;
delete sa.t;
s.t = "zz" + String(o.k);
console.log(s.t, typeof s.t, JSON.stringify(s));
