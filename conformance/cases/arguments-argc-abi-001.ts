// RFC 20260810-indirect-argc-abi S3.2 — env-first faces read
// `arguments.length` off the S1 hidden-ABI argc param instead of the
// AST-injected `__torajs_real_argc`. Covers the three env-first tiers
// (IIFE / closure value / argv face), the head-less tier that stays on
// the injected param, and the I64-argc-into-f64-arith width seam.

// iife tier (length-only body)
console.log((function f3(a: any, b: any, c: any) {
  return arguments.length;
})(1, 2));

// closure value tier (length-only body, called through a binding)
function callWith2(cb: any): any {
  return cb(10, 20);
}
const lenOnly = function (x: any, y: any, z: any) {
  return arguments.length;
};
console.log(callWith2(lenOnly));

// argv face (length + element reads)
const picky = function (a: any, b: any) {
  return "" + arguments.length + ":" + arguments[0] + "," + arguments[2];
};
console.log(callWith2(picky));

// head-less top-level fn (stays on the injected real_argc)
function headless(a: any, b: any, c: any) {
  return arguments.length;
}
console.log(headless(7));

// width seam: I64 hidden argc flowing into f64 arithmetic
console.log((function w(a: any, b: any) {
  return arguments.length * 0.5 + 0.25;
})(1, 2, 3, 4));

// comparison face
console.log((function c(a: any) {
  return arguments.length === 2;
})(1, 2));
