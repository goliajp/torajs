// Adapted from test262: language/expressions/logical-or/S11.11.2_A2.1_T*.js
// Boolean-only; spec's "return first truthy" not in the subset.
function check(): number {
  if ((true || true) !== true) { throw "#1"; }
  if ((true || false) !== true) { throw "#2"; }
  if ((false || true) !== true) { throw "#3"; }
  if ((false || false) !== false) { throw "#4"; }
  let a: boolean = true;
  let b: boolean = false;
  if ((a || b) !== true) { throw "#5"; }
  if ((b || b) !== false) { throw "#6"; }
  return 0;
}
console.log(check());
