// Adapted from test262: language/expressions/bitwise-not/* — `~x` is
// `x ^ -1`. Integer operands only.
function check(): number {
  if (~0 !== -1) { throw "#1"; }
  if (~-1 !== 0) { throw "#2"; }
  if (~5 !== -6) { throw "#3"; }
  if (~-6 !== 5) { throw "#4"; }
  if (~~42 !== 42) { throw "#5"; }
  return 0;
}
console.log(check());
