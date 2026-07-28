// cluster #1 blade 5b — an untyped-param plain fn (no `this`) passed
// as a member-call argument: desugar_implicit_generics turns its
// params into __T<N> TypeVars, so the value use has no concrete
// raw-FnSig instance to carry. The collector wraps it into its
// __forward_* shim, whose closure-shape params default to `any` and
// whose direct forwarding call is the mono site instantiating the
// generic at all-any. Typed named fns keep their raw FnSig paths.
function callbackfn(val, idx, obj) {
  return val > 0;
}
var arr = [1];
console.log(arr.every(callbackfn));

function add1(x) {
  return x + 1;
}
var xs = [1, 2, 3];
console.log(xs.map(add1));
console.log(xs.filter(function pred(v) { return v > 1; }));
console.log(xs.some(callbackfn));
console.log(xs.findIndex(function h(v, i) { return v === 3 && i === 2; }));

// typed named fn stays on its typed path
function dbl(x: number): number {
  return x * 2;
}
console.log(xs.map(dbl));
