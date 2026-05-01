// Adapted from test262: language/expressions/conditional/* — `?:`
// ternary operator. Cond must be Boolean; both branches must have the
// same type.
function abs(n: number): number { return n < 0 ? -n : n; }
function maxOf(a: number, b: number): number { return a > b ? a : b; }
function pickStr(b: boolean): string { return b ? "yes" : "no"; }

function check(): number {
  if (abs(5) !== 5) { throw "#1"; }
  if (abs(-5) !== 5) { throw "#2"; }
  if (abs(0) !== 0) { throw "#3"; }
  if (maxOf(3, 7) !== 7) { throw "#4"; }
  if (maxOf(7, 3) !== 7) { throw "#5"; }
  if (pickStr(true) !== "yes") { throw "#6"; }
  if (pickStr(false) !== "no") { throw "#7"; }
  // Nested ternary
  let x: number = 10;
  let label: string = x < 0 ? "neg" : x === 0 ? "zero" : "pos";
  if (label !== "pos") { throw "#8"; }
  return 0;
}
console.log(check());
