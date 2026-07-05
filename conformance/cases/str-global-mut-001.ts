// L3b #19 / chunk 558 — mutable Str top-level globals. The K.6
// mutable-refcount exclusion kept `let g = "start"` out of the
// globals map (main-local home), so a named-fn assignment hit
// "ssa-lower: assign to unknown ident" while the I64 twin worked.
// Strings have no in-place mutation methods, so the Arr/Obj
// writeback concern doesn't apply: promote the slot, run
// borrow-inc → load-old → store-new → drop-old on assignment.
let g = "start";

function bump(): void {
  g = g + "-x";
}

function setTo(v: string): void {
  g = v;
}

function read(): string {
  return g;
}

bump();
console.log(g);
setTo("fresh");
console.log(read());

// main-scope assignment through the same global path.
g = g + "!";
console.log(g);

// self-assign: borrow inc lands before the old value's dec.
g = g;
console.log(g);

// local = global re-assignment is a borrow — both stay live.
let alias = "";
alias = g;
console.log(alias);
console.log(g);

// annotated non-literal init form (K.3 explicit-annotation path).
let h: string = "he" + "ap";
function grow(): void {
  h = h + "-more";
}
grow();
grow();
console.log(h);

// loop-heavy churn: each round drops the previous cell.
for (let i = 0; i < 5; i++) {
  g = g + "." + i.toString();
}
console.log(g);
