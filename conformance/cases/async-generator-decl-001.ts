// RFC 20260713-generator-fn-value-substrate blade 4 (step 1) — async
// generator declarations get their spec §27.6 shape: `ag()` answers
// the generator object DIRECTLY (pre-fix desugar_async Promise-wrapped
// the factory, so ag() was Promise<GenObj> — the exact inversion), and
// each next()/return()/throw() answers a Promise of the step struct
// (mangled __cm___Gen_*__ methods registered into async_fns so the
// class-method async rewrite shapes them).
//
// Also covered: the exhausted-step value is `undefined` (the any-slot
// zero value), not 0 — default_init_for_type("any") now answers the
// undefined ident. A single async chain keeps tr's eager-fire
// completion order aligned with bun's microtask order.

// Sync generator first: exhausted step value is undefined.
function* sg() {
  yield 7;
}
const sit = sg();
console.log(sit.next().value);           // 7
const done = sit.next();
console.log(done.value, done.done);      // undefined true

async function* ag() {
  yield 1;
  yield 2;
}

async function* pow2(n: number) {
  yield n * n;
}

async function main() {
  const it = ag();
  const r1 = await it.next();
  console.log(r1.value, r1.done);        // 1 false
  const r2 = await it.next();
  console.log(r2.value, r2.done);        // 2 false
  const r3 = await it.next();
  console.log(r3.value, r3.done);        // undefined true
  const p = pow2(6);
  const r4 = await p.next();
  console.log(r4.value, r4.done);        // 36 false
}
main();
