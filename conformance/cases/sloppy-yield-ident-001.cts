// §12.7.2 — outside a generator, `yield` is a valid identifier under
// the sloppy goal (.cts): binding name, IdentifierReference,
// assignment target. The strict goal rejects at the prelude gate
// (triage_yield_idents); this fixture drives the sloppy admission.
var yield = 4;
console.log(yield);

// IdentifierReference in an initializer (the t262 dstr header shape).
var x;
var vals = [];
var result;
result = [ x = yield ] = vals;
console.log(x);
console.log(result === vals);

// Assignment target + arithmetic operand.
yield = yield * 2;
console.log(yield);

// Reference from a non-generator function body (closure capture).
function g() {
  return yield + 1;
}
console.log(g());

// A fresh inner binding shadows the outer one.
function h() {
  var yield = 100;
  return yield - 1;
}
console.log(h());

console.log(typeof yield);
