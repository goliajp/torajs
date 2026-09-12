// Rotation 636 — a `yield` in a conditionally evaluated operand: a
// short-circuit rhs (`&&` / `||` / `??`) or a ternary branch. The
// operator folds into a temp; the operand's yields run only on the
// path that reaches it, and the operator's own value is still the
// expression's value.
function* g1(a) { var r = a && (yield "and"); console.log("and", r); }
var i1 = g1(0); console.log(i1.next().value); console.log(i1.next().done);
var i2 = g1(1); console.log(i2.next().value); console.log(i2.next(5).done);
function* g2(a) { var r = a || (yield "or"); console.log("or", r); }
var i3 = g2(7); console.log(i3.next().value); console.log(i3.next().done);
var i4 = g2(0); console.log(i4.next().value); console.log(i4.next("v").done);
function* g3(a) { var r = a ?? (yield "nn"); console.log("nn", r); }
var i5 = g3(0); console.log(i5.next().value); console.log(i5.next().done);
var i6 = g3(null); console.log(i6.next().value); console.log(i6.next(9).done);
function* g4(c) { var r = c ? (yield "t") : (yield "e"); console.log("tern", r); }
var i7 = g4(true); console.log(i7.next().value); console.log(i7.next("T").done);
var i8 = g4(false); console.log(i8.next().value); console.log(i8.next("E").done);
function* g5(a, b) { var r = a && (b || (yield "deep")); console.log("deep", r); return r; }
var i9 = g5(1, 0); console.log(i9.next().value); console.log(i9.next(3).value);
var i10 = g5(1, 2); console.log(i10.next().value);
function* g6(c) { var n = 0; var r = c ? (yield* [1, 2]) : n; console.log("star", r); }
var i11 = g6(true); console.log(i11.next().value, i11.next().value, i11.next().done);
