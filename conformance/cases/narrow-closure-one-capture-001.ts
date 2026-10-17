// Of two narrowed captures only the one the body needs non-null keeps
// its narrow; the other is read at `Node | null` and may be nulled.
type Node = { v: number };
function mk(): Node {
  return { v: 7 };
}
function g(n: Node): number {
  return n.v;
}

function h(a: Node | null, b: Node | null): number {
  let q: Node | null = a;
  let r: Node | null = b;
  if (q === null) q = mk();
  if (r === null) r = mk();
  const f = (): number => g(q) + (r === null ? 1 : 0);
  r = null;
  return f();
}
console.log(h(null, null), h({ v: 2 }, { v: 5 }));
