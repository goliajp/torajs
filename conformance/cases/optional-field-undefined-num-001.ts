// RFC 20260710-optional-undefined-repr C4 — optional number/boolean
// fields materialize as Any slots: undefined round-trips through eq /
// print / truthiness, values box and unbox transparently, and a plain
// `any` field stops mis-reading raw scalars as NaN-box cells.
type O = { tag?: string; n?: number; cb?: () => string };
const o: O = { tag: undefined, n: undefined, cb: undefined };
console.log(o.tag === undefined, o.n === undefined, o.cb === undefined);
console.log(o.n === null);
console.log(o.tag, o.n, o.cb);

type N = { n?: number };
const p: N = { n: 5 };
console.log(p.n === undefined, p.n);
const q: N = {};
console.log(q.n === undefined, q.n);
q.n = 7;
console.log(q.n, q.n === 7);
q.n = undefined;
console.log(q.n === undefined, q.n);
if (q.n) { console.log("truthy"); } else { console.log("falsy"); }

type B = { f?: boolean };
const b: B = { f: true };
console.log(b.f, b.f === undefined);
b.f = undefined;
console.log(b.f, b.f === undefined);
const c: B = {};
console.log(c.f === undefined, c.f);

type A = { v: any };
const a: A = { v: 5 };
console.log(a.v, a.v === 5);
a.v = undefined;
console.log(a.v === undefined, a.v === null, a.v);
a.v = null;
console.log(a.v === null, a.v);

type G = { v?: number; label: string };
const g: G = { v: 42, label: "g" };
console.log(g.v === undefined, g.v, g.label);
const h: G = { v: undefined, label: "h" };
console.log(h.v === undefined, h.v, h.label);
