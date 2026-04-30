// Adapted from test262: language/expressions/less-than-or-equal +
// greater-than-or-equal — boundary cases at equality.
function check(): number {
  if ((5 <= 5) !== true) { throw "#1"; }
  if ((5 >= 5) !== true) { throw "#2"; }
  if ((4 <= 5) !== true) { throw "#3"; }
  if ((5 <= 4) !== false) { throw "#4"; }
  if ((6 >= 5) !== true) { throw "#5"; }
  if ((4 >= 5) !== false) { throw "#6"; }
  if ((-1 <= 0) !== true) { throw "#7"; }
  if ((0 >= -1) !== true) { throw "#8"; }
  return 0;
}
console.log(check());
