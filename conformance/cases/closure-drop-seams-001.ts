// RFC 20260824-s2-5 刀 4 A5 — a closure env-drop's two speculative
// legs sit behind link seams: the props-bag release (a user closure
// only grows a bag through `__torajs_closure_props_attach`: `f.x =
// v`, `Object.setPrototypeOf(f, …)`, a fnprops bag migrating onto
// its cell) and the cycle-buffer scrub (only `__torajs_cycle_buffer`
// sets FLAG_BUFFERED — a closure captured into a cycle). Every leg
// below must run for real: a stripped seam answers a named
// TypeError, never a silent leak.
const tagged = (n: number): number => n + 1;
(tagged as any).count = 3;
console.log((tagged as any).count, tagged(1));

const proto = { hello: "hi" };
const viaProto = (): number => 7;
Object.setPrototypeOf(viaProto, proto);
console.log(Object.getPrototypeOf(viaProto) === proto, viaProto());

function named(v: number): number {
  return v * 2;
}
(named as any).label = "n";
const boundNamed: any = named;
console.log(boundNamed.label, named(4));

// a closure cycle: env -> box -> env, dropped 200 times
for (let i = 0; i < 200; i++) {
  let self: any = null;
  const cyc = () => self;
  self = cyc;
  if (i === 199) console.log(typeof cyc(), cyc() === cyc);
  self = null;
}
console.log("done");
