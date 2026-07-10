// RFC 20260710-optional-undefined-repr C2b — refcounted pointer
// slots (Obj / Arr / Closure) adopt the generic Tag::Undefined
// oddball cell: undefined round-trips through eq / print /
// truthiness / JSON key-skip, explicit null keeps its exact
// behavior, and live cells drop through the nullish-guarded
// stations unchanged.
type Inner = { a: number };
type O = { inner?: Inner; xs?: number[] };
const o: O = { inner: undefined, xs: undefined };
console.log(o.inner === undefined, o.xs === undefined);
console.log(o.inner === null, o.xs === null);
console.log(o.inner, o.xs);
console.log(o.inner, 1);
if (o.inner) { console.log("truthy"); } else { console.log("falsy"); }
console.log(JSON.stringify(o));

const p: O = { inner: { a: 7 }, xs: [1, 2] };
console.log(p.inner, p.inner === undefined);
console.log(p.xs);
console.log(JSON.stringify(p));
if (p.inner) { console.log("live-truthy"); }

const q: O = { inner: null, xs: null };
console.log(q.inner, q.inner === null, q.inner === undefined);
console.log(JSON.stringify(q));

const w: O = {};
console.log(w.inner === undefined, w.xs === undefined, w.inner, w.xs);

const m: O = { inner: { a: 1 }, xs: [3] };
m.inner = undefined;
m.xs = undefined;
console.log(m.inner === undefined, m.xs === undefined, m.inner, m.xs);
m.inner = { a: 2 };
console.log(m.inner === undefined, m.inner);

const a: any = o.inner;
console.log(a === undefined, a);
