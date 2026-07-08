// Chunk 690 — static Object.fromEntries fold (T-09): a pairs ARRAY
// LITERAL with string-literal keys folds to the equivalent object
// literal before typecheck and rides the anonymous-struct lanes end
// to end. Dynamic entries / duplicate keys / empty / non-literal
// keys stay on the existing loud reject.
const o = Object.fromEntries([
  ["a", 1.5],
  ["b", 2.5],
]);
console.log(o.a, o.b);
// mixed value types
const m = Object.fromEntries([
  ["s", "hi"],
  ["n", 42],
  ["f", true],
]);
console.log(m.s, m.n, m.f);
// value exprs evaluate pair by pair
function t(x: number): number {
  console.log("eval", x);
  return x * 2;
}
const e = Object.fromEntries([
  ["p", t(1)],
  ["q", t(2)],
]);
console.log(e.p, e.q);
// whole-object print
console.log(o);
// nested array value
const nv = Object.fromEntries([["xs", [1, 2, 3]]]);
console.log(nv.xs.length, nv.xs[2]);
