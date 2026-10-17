// A closure whose body checks against the declared `Node | null` is
// typed at it, so a later null store stays legal and the call sees it.
type Node = { v: number };
function mk(): Node {
  return { v: 7 };
}
function g(n: Node): number {
  return n.v;
}

function h(a: Node | null): number {
  let q: Node | null = a;
  if (q === null) q = mk();
  const f = (): number => (q !== null ? g(q) : 0);
  q = null;
  return f();
}
console.log(h(null), h({ v: 3 }));
