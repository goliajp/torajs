// Adapted from test262: object literal with multi-level nesting.
type Inner = { count: number };
type Middle = { tag: string, inner: Inner };
type Outer = { id: number, mid: Middle };

function check(): number {
  let o: Outer = {
    id: 1,
    mid: {
      tag: "alpha",
      inner: { count: 42 }
    }
  };
  if (o.id !== 1) { throw "#1"; }
  if (o.mid.tag !== "alpha") { throw "#2"; }
  if (o.mid.inner.count !== 42) { throw "#3"; }
  o.mid.inner.count = 100;
  if (o.mid.inner.count !== 100) { throw "#4"; }
  return 0;
}
console.log(check());
