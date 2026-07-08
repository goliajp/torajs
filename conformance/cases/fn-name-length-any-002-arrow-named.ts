// chunk 720 — NamedEvaluation: let-bound arrows answer the binding
// name / arity through the fn-addr registry.
// Recorded divergence (kept OUT of this fixture): an arrow escaping
// its declaring fn (`return inner`) keeps its binding name in tr
// (spec §8.4.5, node agrees) while bun answers "" — bun transpiler
// quirk, spec is the source of truth.
const add = (a: number, b: number) => a + b;
const t: any = add;
console.log(t.name, t.length, typeof t);
const one = (x: number) => x + 1;
const u: any = one;
console.log(u.name, u.length);
// fn-body nested binding, read in scope (bun agrees here)
function outer2(): number {
  const inner = (q: number, w: number, e: number) => q + w + e;
  const iv: any = inner;
  console.log(iv.name, iv.length);
  return inner(1, 2, 3);
}
console.log(outer2());
// default-param arity clamp
const dflt = (a: number, b: number = 5) => a + b;
const d: any = dflt;
console.log(d.name, d.length);
// zero-param arrow
const nil = () => 42;
const z: any = nil;
console.log(z.name, z.length);
// bind over a named arrow: prefix + subtract now that metadata exists
const bd = t.bind(null, 1);
console.log(bd.name, bd.length);
// calls still work
console.log(add(2, 3), (t as any)(4, 5), bd(9));
