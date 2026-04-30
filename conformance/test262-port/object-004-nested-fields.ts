// Adapted from test262: nested object literal — verifies deep field reads
// resolve through intermediate Obj layers.
type Inner = { v: number };
type Outer = { name: string, inner: Inner };

function check(): number {
  let o: Outer = { name: "hi", inner: { v: 42 } };
  if (o.name !== "hi") { throw "#1"; }
  if (o.inner.v !== 42) { throw "#2"; }
  o.inner.v = 100;
  if (o.inner.v !== 100) { throw "#3"; }
  return 0;
}
console.log(check());
