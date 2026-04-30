// Adapted from test262: language/expressions/less-than & greater-than (number).
function check(): number {
  if ((1 < 2) !== true) { throw "#1"; }
  if ((2 < 1) !== false) { throw "#2"; }
  if ((1 < 1) !== false) { throw "#3"; }
  if ((2 > 1) !== true) { throw "#4"; }
  if ((1 > 2) !== false) { throw "#5"; }
  if ((1 <= 1) !== true) { throw "#6"; }
  if ((1 >= 1) !== true) { throw "#7"; }
  if ((0 - 1 < 1) !== true) { throw "#8: -1 < 1"; }
  return 0;
}
console.log(check());
