// Adapted from test262: built-ins/Number/isSafeInteger/* — true iff
// n is an integer-valued number within ±(2^53 - 1). Same i64/f64
// dispatch shape as the rest of the Number predicates: integer args
// only need a range check; f64 args also test for finite + integer-
// valued.
function check(): number {
  // Safe integers.
  if (Number.isSafeInteger(0) !== true) { throw "#1"; }
  if (Number.isSafeInteger(42) !== true) { throw "#2"; }
  if (Number.isSafeInteger(-100) !== true) { throw "#3"; }
  if (Number.isSafeInteger(Number.MAX_SAFE_INTEGER) !== true) { throw "#4"; }
  if (Number.isSafeInteger(Number.MIN_SAFE_INTEGER) !== true) { throw "#5"; }

  // Non-integers / non-finite.
  if (Number.isSafeInteger(3.14) !== false) { throw "#6: fractional"; }
  if (Number.isSafeInteger(0.5) !== false) { throw "#7"; }
  if (Number.isSafeInteger(Number.NaN) !== false) { throw "#8"; }
  if (Number.isSafeInteger(Number.POSITIVE_INFINITY) !== false) { throw "#9"; }
  if (Number.isSafeInteger(Number.NEGATIVE_INFINITY) !== false) { throw "#10"; }
  return 0;
}
console.log(check());
