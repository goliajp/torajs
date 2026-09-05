// A top-level binding that ALIASES a closure binding, read from a
// function declaration. A named fn body has no capture machinery, so
// it reads top-level bindings through the globals table — and a
// binding only lands there if both the checker and the lowering infer
// a slot for it, from the same spelling. Neither did for `const c =
// k`, so this ordinary program was a hard reject ("unknown identifier
// c") while `const c: any = k` — the same values, one annotation —
// worked.
//
// The alias holds the identical value, so it registers under the
// identical `__fn(...)` spelling the closure binding itself uses.
let half = (a: number) => a / 4;
const alias = half;
function viaAlias() {
  return alias(9);
}
// Both homes agree: the main path and the named-fn path read one
// binding, and a fractional result rules out an integer-width slot
// reading f64 bits back as a garbage integer.
console.log(alias(9), viaAlias());

// Returning the alias, rather than calling it.
let tag = (a: number) => "x" + a;
const tagAlias = tag;
function giveTag() {
  return tagAlias;
}
console.log(giveTag()(7));

// Reassigning the SOURCE after the alias is taken does not reach the
// alias — it took the value, not the binding.
let step = (a: number) => a + 1;
const stepAlias = step;
step = (a: number) => a + 2;
function viaStepAlias() {
  return stepAlias(1);
}
console.log(viaStepAlias(), step(1));

// A void signature, and a two-parameter one.
let noop = () => {};
const noopAlias = noop;
let ratio = (a: number, b: number) => a / b + 0.5;
const ratioAlias = ratio;
function useBoth() {
  noopAlias();
  return ratioAlias(9, 2);
}
console.log(useBoth());

// A name declared more than once at the top level is not chased: the
// alias site's `k` need not be the declaration a search happens to
// find first. This one stays main-local, which still works there.
let shadowed = (a: number) => a + 10;
{
  let shadowed = 5;
  console.log(shadowed);
}
const shadowedAlias = shadowed;
console.log(shadowedAlias(1));
