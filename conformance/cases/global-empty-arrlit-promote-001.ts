// Cluster-values follow-up knife — a top-level `var xs = []` (the
// test262 collector idiom) promotes as an `any[]` global (K.6
// empty-array fast path), so named-fn bodies can push into and read
// it. A NESTED empty literal still keeps its parent main-local.

var xs = [];
function fill() {
  (xs as any).push(1);
  (xs as any).push("two");
  (xs as any).push(null);
}
fill();
console.log("len", (xs as any).length);
console.log("elems", (xs as any)[0], (xs as any)[1], (xs as any)[2] === null);

// read side from another fn
function sum() {
  let n = 0;
  for (let i = 0; i < (xs as any).length; i++) {
    if (typeof (xs as any)[i] === "number") n += (xs as any)[i];
  }
  return n;
}
console.log("sum", sum());

// main-side push keeps working after promotion
(xs as any).push(9);
console.log("after-main", (xs as any).length, (xs as any)[3]);

// non-empty literal keeps its typed lane (regression guard)
var typed = [10, 20];
function readTyped() {
  console.log("typed", typed[0] + typed[1]);
}
readTyped();
