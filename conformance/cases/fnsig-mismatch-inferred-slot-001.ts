// A mutable fn slot with NO annotation — its type is the
// initializer's own face.
//
// The mismatch census compared a store against the slot's ANNOTATION,
// so an unannotated slot had nothing to compare and kept the bare
// indirect call shaped by its INIT's face. A later store of a wider
// face then had the callee read an argument register the caller never
// filled: `var x = function () { return 1 }; x = function (a) {
// return a }; x(7)` answered NaN. With no annotation string in play
// the two faces compare directly instead.

function gb(p = 5) { console.log("gb", p); }

// let, init literal, assigned a named declaration.
let a = function () { console.log("a"); };
a();
a = gb;
a();

// var, init literal, assigned a wider literal.
var b = function () { return 1; };
console.log(b());
b = function (n) { return n; };
console.log(b(7));

// An identical face keeps the bare lane — there is nothing to widen,
// and the answer must not change either.
let same = function () { return 1; };
same = function () { return 2; };
console.log(same());
