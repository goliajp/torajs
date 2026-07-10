// chunk 762 — struct-param Nullable-field covariance at the call
// boundary: a `{ inner?: Inner }` param must admit both the
// value-carrying and the explicit-undefined object-literal arg
// shapes (the let-decl lane already did via the assignability
// lattice; the call-arg lane fell to strict equality and rejected
// both).
type Inner = { a: number };
type O = { inner?: Inner; xs?: number[] };
function show(o: O): void {
  console.log(o.inner);
  console.log(o.xs);
}
show({ inner: undefined, xs: undefined });
show({ inner: { a: 3 }, xs: [1, 2] });
const w: O = {};
console.log(w.inner === undefined, w.xs === undefined);
