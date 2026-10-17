// a member narrowed by an early exit stays narrowed after the `if`
type Node = { v: number };
type O = { f: Node | null };
function g(n: Node): number { return n.v }
function h(o: O): number {
  if (o.f === null) return 0;
  return g(o.f);
}
function k(os: O[]): number {
  let s = 0;
  for (const o of os) {
    if (o.f === null) continue;
    s += g(o.f);
  }
  return s;
}
function w(o: O): number {
  if (o.f === null) return -1;
  const a = g(o.f);
  o.f = null;
  return o.f === null ? a : -2;
}
console.log(h({ f: null }), h({ f: { v: 3 } }));
console.log(k([{ f: { v: 1 } }, { f: null }, { f: { v: 5 } }]));
console.log(w({ f: { v: 9 } }));
