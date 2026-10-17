// A nested function declaration reads a binding narrowed before the
// call, with no store after it.
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
  return k();
  function k(): number {
    return g(q);
  }
}
console.log(h(null), h({ v: 3 }));
