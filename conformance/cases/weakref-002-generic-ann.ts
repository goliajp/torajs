// Chunk 629 — `WeakRef<T>` generic annotations resolve (erased to
// the weak-family sibling shape, target type validated then
// dropped), deref() answers a boxed Nullable<Any> the static
// surface can narrow and member-access, and `x !== undefined`
// narrows a Nullable binding the same way `x !== null` always has
// (P1.7 collapse). Pre-fix: `function mk(n: Node): WeakRef<Node>`
// was "unknown return type", and member access on a narrowed deref
// result was a loud reject.
class Node {
  v: number = 0;
}

function mk(n: Node): WeakRef<Node> {
  return new WeakRef(n);
}

const a = new Node();
a.v = 7;
const r = mk(a);
const got = r.deref();
if (got !== undefined) {
  console.log(got.v);
}

// annotated binding form
const r2: WeakRef<Node> = new WeakRef(a);
console.log(r2.deref() === null);
console.log(typeof r2);

// undefined-comparison narrow on a plain Nullable (regex exec hit)
const m = /b(c)/.exec("abcd");
if (m !== undefined && m !== null) {
  console.log(m.length);
  console.log(m[1]);
}
