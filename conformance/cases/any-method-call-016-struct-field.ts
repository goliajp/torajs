// L3b #9 (chunk 524) — Tag::Obj struct-cell receivers in the any
// method-call dispatcher: a nested ObjectLit inside an any init
// lowers as an anon struct, so `n.inner.op(1, 1)` probes the
// class-layouts field metadata instead of a dynobj table. Covers
// the chained call, an intermediate any variable, an any-typed
// closure slot, multi-level nesting, and the catchable miss shapes.
const n: any = { inner: { op: (a: number, b: number) => a + b } };
console.log(n.inner.op(1, 1));
const i = n.inner;
console.log(i.op(20, 22));
const f = i.op;
console.log(f(3, 4));
const deep: any = { a: { b: { mul: (x: number, y: number) => x * y } } };
console.log(deep.a.b.mul(6, 7));
const cb: any = (x: number) => x + 1;
const holder: any = { pair: { fn: cb, tag: "t" } };
console.log(holder.pair.fn(41));
console.log(holder.pair.tag);
try {
  i.missing(1);
} catch (e) {
  console.log("missing struct field threw");
}
try {
  const data: any = { inner: { v: 9 } };
  data.inner.v(1);
} catch (e) {
  console.log("non-closure struct field threw");
}
