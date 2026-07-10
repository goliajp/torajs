// chunk 750 — fresh-owned string operands of the inline `===
// "literal"` fast path and fresh-owned switch scrutinees now
// release after the compare (both early paths skipped lower_binop's
// fresh-owned drop dance, stranding one cell per compare). Value
// semantics pinned here; the leak itself is churn-probed (q1-q6,
// 15.9MB -> 6.3MB flat).
function mk(): string { return "ab" + "c"; }
console.log(mk() === "abc", mk() !== "abc", mk() === "xyz");
const bound = "ab" + "c";
console.log(bound === "abc");
console.log(bound.length);
function pick(i: number): string { return "k" + i; }
let acc = 0;
for (let i = 0; i < 6; i++) {
  switch (pick(i % 3)) {
    case "k0": acc += 1; break;
    case "k1": acc += 2; break;
    default: acc += 3;
  }
}
console.log(acc);
function classify(i: number): number {
  switch ("k" + (i % 2)) {
    case "k0": return 10;
    default: return 20;
  }
}
console.log(classify(0), classify(1));
const s = "dyn" + "amic";
switch (s) {
  case "dynamic": console.log("hit"); break;
  default: console.log("miss");
}
