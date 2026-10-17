// Lazy initialization: nothing stores into the binding after the
// closure exists, so it reads the narrow.
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
  return f();
}
console.log(h(null), h({ v: 3 }));
