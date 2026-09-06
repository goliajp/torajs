// §8.1.1.1, second half — the dead-zone read whose binding is a `let`,
// or whose literal is a string. Both are excluded from the ident
// lane's inline-literal answer for reasons that only apply to a read
// that RETURNS (a later write must be visible; interning per read site
// costs an allocation each time). A read that raises first does
// neither, so it takes the literal's shape and the raise stands —
// where before, the whole program aborted at compile time.
try {
  console.log(count);
} catch (e) {
  console.log((e as Error).message);
}
let count = 5;
count = 9;
console.log(count);

try {
  console.log(label);
} catch (e) {
  console.log((e as Error).message);
}
const label = "ready";
console.log(label, label.length);

try {
  console.log(typeof note);
} catch (e) {
  console.log((e as Error).message);
}
let note = "draft";
note = "final";
console.log(note);

// The relaxation is scoped to marked reads: an ordinary `let` read
// still loads, so a write before it is visible.
let total = 1;
total = 42;
console.log(total);
function readTotal(): number {
  return total;
}
console.log(readTotal());
