// Adapted from test262: language/statements/for-in/* — for-in over a
// struct iterates field names as strings. tr desugars at parse time
// into `Object.keys(obj)` + the existing for-of pipeline; the keys
// array is itself a compile-time constant. Body sees `k: string` per
// field-declaration order.
type Pt = { x: number, y: number, label: string };

function check(): number {
  let p: Pt = { x: 5, y: 7, label: "origin" };
  let collected: string[] = [];
  for (let k in p) {
    collected.push(k);
  }
  if (collected.length !== 3) { throw "#1"; }
  if (collected[0] !== "x") { throw "#2"; }
  if (collected[1] !== "y") { throw "#3"; }
  if (collected[2] !== "label") { throw "#4"; }
  return 0;
}
console.log(check());
