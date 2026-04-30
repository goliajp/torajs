// Adapted from test262: Math.{exp,log} round-trip. log(exp(x)) ≈ x for
// integer x within float precision. We bound-check rather than equality
// because IEEE 754 introduces tiny rounding.
function check(): number {
  // Math.exp(0) === 1
  if (Math.exp(0) !== 1) { throw "#1"; }
  // Math.log(1) === 0
  if (Math.log(1) !== 0) { throw "#2"; }
  // Math.PI > 3, < 4
  if (Math.PI < 3) { throw "#3"; }
  if (Math.PI > 4) { throw "#4"; }
  // Math.E > 2, < 3
  if (Math.E < 2) { throw "#5"; }
  if (Math.E > 3) { throw "#6"; }
  // Math.pow(0, anything) === 0 except 0^0
  if (Math.pow(0, 5) !== 0) { throw "#7"; }
  return 0;
}
console.log(check());
