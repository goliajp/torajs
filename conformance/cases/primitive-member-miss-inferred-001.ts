// §10.1.8.1 — a property MISS on a primitive receiver is `undefined`,
// not a refusal. The checker already says so for an INFERRED anchor
// (rotation 620: the Struct a specialization body pins is an anchor,
// not a contract), but the member ladder in lowering had no arm for
// the number and boolean reprs, so it panicked:
//
//     function f(n) { return n.nope; }   f(1)
//         not yet supported: ssa-lower: member access on non-object
//                            I64 (.nope)
//
// The very same function answered `undefined` for `"x"` and `{x:1}` —
// those reprs were on the list. A DECLARED `n: number` never reaches
// the ladder: the checker refuses it first, which is what tsc does
// too (TS2339), so this changes nothing about the declared spelling.

function miss(n: any) {
  return n.nope;
}
console.log(miss(1), miss(true), miss("x"), miss({ x: 1 }), miss(1n));
console.log(miss(1.5), miss(false), miss([1, 2]));

function missIndex(n: any) {
  return n["nope"];
}
console.log(missIndex(1), missIndex(true));

function missEq(n: any) {
  return n.nope === undefined;
}
console.log(missEq(1), missEq(true), missEq(2.5));

function missTypeof(n: any) {
  return typeof n.nope;
}
console.log(missTypeof(1), missTypeof(true));

// the properties that DO exist keep answering off their own lanes
function len(s: any) {
  return s.length;
}
console.log(len("abc"), len([1, 2]));
console.log((1).toFixed(2), (255).toString(16), true.toString());
