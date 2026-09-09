// §10.1.8.1 — indexing a primitive is an ordinary [[Get]] through its
// wrapper: `(1)[0]` and `true[0]` answer undefined, and a string
// answers its character. The index ladder routes by receiver SHAPE
// and its any-lane list carried every heap shape with an expando bag
// and none of the primitive reprs, so a specialization body's
// inferred Number or Boolean anchor died:
//
//     function f(n) { return n[0]; }   f(1)
//         not yet supported: ssa-lower: index access on non-array
//                            type I64
//
// `(1 as any)[0]` answered undefined off the runtime dispatcher all
// along — the same dispatcher this now boxes onto.

function at0(n: any) {
  return n[0];
}
console.log(at0(1), at0(true), at0(2.5), at0(1n));
console.log(at0("abc"), at0([7, 8]));

function atKey(n: any, k: any) {
  return n[k];
}
console.log(atKey(1, "nope"), atKey(true, "nope"), atKey(1, 0));
console.log(atKey("abc", 1), atKey("abc", "length"));

function eqUndef(n: any) {
  return n[0] === undefined;
}
console.log(eqUndef(1), eqUndef(true), eqUndef("abc"));

function typeOfAt(n: any) {
  return typeof n[0];
}
console.log(typeOfAt(1), typeOfAt(true), typeOfAt("abc"));
