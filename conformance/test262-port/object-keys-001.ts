// Adapted from test262: built-ins/Object/keys/* — `Object.keys(obj)`
// returns the field names as a string[]. tr emits the array as a
// compile-time constant from the static struct layout — zero runtime
// reflection cost (no per-instance type tag, no field-info table
// lookup).
type Pt = { x: number, y: number };
type Bag = { name: string, count: number, label: string };

function check(): number {
  let p: Pt = { x: 5, y: 7 };
  let pk = Object.keys(p);
  if (pk.length !== 2) { throw "#1"; }
  if (pk[0] !== "x") { throw "#2"; }
  if (pk[1] !== "y") { throw "#3"; }

  let b: Bag = { name: "alpha", count: 3, label: "ok" };
  let bk = Object.keys(b);
  if (bk.length !== 3) { throw "#4"; }
  if (bk[0] !== "name") { throw "#5"; }
  if (bk[1] !== "count") { throw "#6"; }
  if (bk[2] !== "label") { throw "#7"; }

  // Independent calls produce independent arrays.
  let pk2 = Object.keys(p);
  if (pk2[0] !== "x") { throw "#8"; }
  return 0;
}
console.log(check());
