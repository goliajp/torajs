// Adapted from test262: built-ins/Object/values/* — `Object.values(obj)`
// returns the field values as an array of the homogeneous element type.
// tr only allows homogeneous structs (all fields same type) since the
// resulting array layout is closed-shape; mixed-type structs error at
// typecheck.
type Counts = { a: number, b: number, c: number };
type Labels = { x: string, y: string };

function check(): number {
  let c: Counts = { a: 10, b: 20, c: 30 };
  let cv = Object.values(c);
  if (cv.length !== 3) { throw "#1"; }
  if (cv[0] !== 10) { throw "#2"; }
  if (cv[1] !== 20) { throw "#3"; }
  if (cv[2] !== 30) { throw "#4"; }

  let l: Labels = { x: "alpha", y: "beta" };
  let lv = Object.values(l);
  if (lv.length !== 2) { throw "#5"; }
  if (lv[0] !== "alpha") { throw "#6"; }
  if (lv[1] !== "beta") { throw "#7"; }

  // Aggregate over values.
  let sum: number = 0;
  for (let v of cv) { sum += v; }
  if (sum !== 60) { throw "#8"; }

  return 0;
}
console.log(check());
