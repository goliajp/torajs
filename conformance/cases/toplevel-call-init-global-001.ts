// S2.35 — an un-annotated toplevel let with a call-result init that
// the shape inference can't type promotes to an Any global, so
// named-fn bodies can read it (the test262 IIFE-iterator idiom).
// Shape-typed calls (simple ret ann / Symbol()) keep their exact
// slot — the fallback never demotes them.
let g = (function* () { yield 1; yield 2; })();
function drain() {
  let total = 0;
  for (const v of g) { total += v; }
  return total;
}
console.log(drain());

let it = (function* () { yield 41; })();
function first() { return it.next().value; }
console.log(first());

function mk(): number { return 7; }
let n = mk();
let s = Symbol();
function reads() { return n; }
function readsym() { return typeof s; }
console.log(reads(), readsym());
