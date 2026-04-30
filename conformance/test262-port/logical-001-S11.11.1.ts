// Adapted from test262: language/expressions/logical-and/S11.11.1_A2.1_T*.js
// Boolean-only; spec's "return last truthy value" with non-boolean operands
// isn't in the subset.
function check(): number {
  if ((true && true) !== true) { throw "#1"; }
  if ((true && false) !== false) { throw "#2"; }
  if ((false && true) !== false) { throw "#3"; }
  if ((false && false) !== false) { throw "#4"; }
  let a: boolean = true;
  let b: boolean = false;
  if ((a && b) !== false) { throw "#5"; }
  if ((a && a) !== true) { throw "#6"; }
  return 0;
}
console.log(check());
