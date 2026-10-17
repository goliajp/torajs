// A closure that needs the narrow keeps it when every later store is
// non-null.
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
  const f = (): number => g(q);
  q = { v: 11 };
  return f();
}
console.log(h(null), h({ v: 3 }));
