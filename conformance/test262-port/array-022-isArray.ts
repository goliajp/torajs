// Adapted from test262: built-ins/Array/isArray/* — compile-time
// static check. tr's typed subset knows the static type of every
// expression, so Array.isArray collapses to a constant true/false at
// lower time (no runtime check, no allocation).
function check(): number {
  let xs: number[] = [1, 2, 3];
  let ys: string[] = ["a"];
  let empty: number[] = [];

  if (Array.isArray(xs) !== true) { throw "#1"; }
  if (Array.isArray(ys) !== true) { throw "#2"; }
  if (Array.isArray(empty) !== true) { throw "#3"; }

  let n: number = 42;
  let s: string = "hi";
  let b: boolean = true;
  if (Array.isArray(n) !== false) { throw "#4"; }
  if (Array.isArray(s) !== false) { throw "#5"; }
  if (Array.isArray(b) !== false) { throw "#6"; }

  // Direct literal.
  if (Array.isArray([1, 2]) !== true) { throw "#7"; }
  if (Array.isArray("inline") !== false) { throw "#8"; }
  return 0;
}
console.log(check());
