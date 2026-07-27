// S2.24 刀 4 — CoverInitializedName (§13.2.5.1): `{ x = D }` is
// legal only when the object literal re-reads as an assignment
// pattern. The parser stores the field as the cover assignment
// `x = D` (the exact shape the pattern walk's `f: y = D` default arm
// consumes) and records it; a literal surviving to expression
// position early-errors at check. The default-guarded field load is
// lenient on a STATIC miss — it rides the any-member runtime GetV,
// because a prefix-widened heterogeneous array element may really
// carry the field the anchor layout lacks.

// 1) assignment-position obj pattern with shorthand defaults
let x = 0, y = 0;
({ x = 5, y = 6 } = { x: 1 });
console.log(x, y);

// 2) nested: obj pattern with default inside an array pattern
let a = 0;
[{ a = 9 } = {}] = [{}];
console.log(a);

// 3) chained through a cover pattern — value is the RHS reference
let r;
r = { x = 50 } = { x: 2, y: 3 };
console.log(x, r.y);

// 4) for-of bare obj head; the prefix-widen face — `{}` anchors the
//    element type but the wider element really has `b`
let b = 0;
for ({ b = 77 } of [{}, { b: 3 }]) {
  console.log(b);
}

// 5) declaration lane shares the load recipe: absent field + default
let { m = 5 } = {};
console.log(m);
const { p = 1, q = 2 } = { p: 9 };
console.log(p, q);
let [{ n = 3 } = {}] = [];
console.log(n);
