// L3b ③ — an implicit-generic fn (unannotated params carry fresh
// TypeVars) escaping as a VALUE: the binding's direct call is a
// CallIndirect with no mono channel, so the TypeVar face rejected
// every argument ("expected TypeVar" loud). The checker widens the
// binding's face all-TypeVar→Any, clones an all-`any` spec
// (`$$anywv`), and the ident lowering takes the clone's address.
function f(a) {
  return a * 2;
}
const g = f;
console.log(g(5)); // 10
function m(x, y) {
  return x + y;
}
const n = m;
console.log(n(3, 4)); // 7
console.log(n("a", "b")); // ab (any lane: the clone re-infers per call)
// the ORIGINAL stays directly callable alongside the escape
console.log(f(21)); // 42
console.log(m(1, 1)); // 2
// typed fn escape keeps its existing (already working) channel
function t(a: number) {
  return a * 3;
}
const tt = t;
console.log(tt(5)); // 15
