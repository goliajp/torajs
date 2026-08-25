// ns-static fallback narrowing (RFC 20260824-s2-5): reified
// namespace-static cells no longer punt the whole stub judgment —
// the per-static table models each cell kernel's re-dispatch
// surface. Behavior must hold under the narrowed stub set:
// coercion through a cell call, a minting static's exotic family,
// promise/iter walks, and the pure-read cells.
const f = Math.max;
console.log(f(1, 2, 3));
const coerce: any = { valueOf() { return 9; } };
console.log(f(coerce, 1)); // OrdinaryToPrimitive inside the cell kernel
const r = Math.random;
console.log(r() >= 0 && r() < 1);
const pint = Number.parseInt;
console.log(pint("42"));
const isArr = Array.isArray;
console.log(isArr([1]), isArr("no"));
const keys = Object.keys;
console.log(keys({ a: 1, b: 2 }));
const sfor = Symbol.for;
const sym = sfor("k");
console.log(typeof sym);
// the promise statics are receiver-channel cells: a detached call
// has an undefined |this| and the spec step-1/2 TypeError is the
// REAL kernel answer — its message proves the promise arm is the
// kernel, not a stub reject.
const pres = Promise.resolve;
try {
  pres(5);
} catch (e) {
  console.log("detached", (e as any).message);
}
