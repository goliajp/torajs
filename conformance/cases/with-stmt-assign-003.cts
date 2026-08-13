// ES §14.11 `with` — RFC 20260814 刀 2: assignment, plain and compound.
//
// A compound assignment reaches the desugar as `n = n op v` with a
// CLONED left operand (the parser's own desugar). That clone is the
// same reference the write resolves, not a second one, so it is filled
// in per branch rather than guarded separately — which is what keeps
// §9.1.1.2.1 HasBinding evaluated ONCE for the whole compound, as the
// spec's single ResolveBinding requires.
//
// The `grow` block below is what a second, independent test would get
// wrong: evaluating the right-hand side adds the property, so a guard
// taken again after it would send the write to the object while the
// read had already come from the outer binding.

var o: any = { a: 1, c: 10 };
var a = 100;
var b = 200;

with (o) {
  a = 2; // object carries it
  b = 3; // it does not — outer binding
  c += 5; // compound: read and write must agree on one binding
}
console.log(o.a, a, b, o.c);

// The outer binding is what a compound lands on when the object does
// not carry the name — and the object must not grow one.
var d = 7;
with (o) {
  d *= 3;
}
console.log(d, o.d);

// Single-resolution check: the right-hand side adds the property
// between what would be two separate membership tests.
var grow: any = {};
var seen = "outer";
with (grow) {
  // a comma expression, not an IIFE: a nested function body inside a
  // `with` is 刀 4 and is refused by name until then.
  seen = (grow.seen = "object", "written");
}
console.log(seen, grow.seen);
