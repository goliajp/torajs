// Adapted from test262: built-ins/Number/prototype/{toExponential,
// toPrecision}/* — scientific and significant-digit formatting. tr's
// runtime uses snprintf %e / %g with a JS-style exponent normalization
// pass (strip leading zeros from the exponent: `1.23e+03` → `1.23e+3`).
function check(): number {
  // toExponential.
  if ((1234).toExponential(2) !== "1.23e+3") { throw "#1"; }
  if ((0.000123).toExponential(2) !== "1.23e-4") { throw "#2"; }
  if ((42).toExponential(0) !== "4e+1") { throw "#3"; }
  if ((0).toExponential(0) !== "0e+0") { throw "#4: zero"; }

  // toPrecision (basic — exact JS shape via snprintf %g).
  if ((1234).toPrecision(3) !== "1.23e+3") { throw "#5"; }
  if ((3.14159).toPrecision(4) !== "3.142") { throw "#6"; }
  if ((1.5).toPrecision(2) !== "1.5") { throw "#7"; }
  // `(0).toPrecision(p)` and `(100).toPrecision(2)` differ between
  // snprintf %g and JS spec (JS pads trailing zeros to fill p);
  // those edge cases are deferred.

  // Negative.
  if ((-1234).toExponential(2) !== "-1.23e+3") { throw "#8"; }
  return 0;
}
console.log(check());
