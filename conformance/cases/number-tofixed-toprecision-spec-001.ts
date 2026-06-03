// P12.3 — Number.prototype.toFixed / toPrecision spec edges that
// go beyond number-tofixed-001-default / -002-rounding coverage.
//
// Two spec-conformance bugs fixed in `3a1a71a`:
//
// 1. toFixed: ES §22.1.3.32 says |n| >= 10^21 → ToString(n)
//    (i.e. Ryū exponent form). Pre-fix tora ran the %f path even
//    for huge numbers and emitted "999999999999999868928.00" for
//    (1e21).toFixed(2).
//
// 2. toPrecision: ES §22.1.3.36 requires emitting exactly
//    `precision` significant digits including trailing zeros.
//    Pre-fix tora called `strip_trailing_zeros_in_frac` (ported
//    from the C subset) which destroyed the trailing zeros and
//    diverged from spec in three classes (e+ form, e- form, 0 input).

// ---- toFixed >= 1e21 → ToString form ----
console.log((1e21).toFixed(0));      // "1e+21"
console.log((1e21).toFixed(2));      // "1e+21"
console.log((-1e21).toFixed(0));     // "-1e+21"
console.log((1.5e21).toFixed(2));    // "1.5e+21"

// ---- toFixed common rounding (regression coverage) ----
console.log((1.005).toFixed(2));     // "1.00" (IEEE round-half-to-even via f64)
console.log((1.235).toFixed(2));     // "1.24"
console.log((-1.5).toFixed(0));      // "-2"  (half-away-from-zero)
console.log((0.000001).toFixed(20)); // "0.00000100000000000000"

// ---- toPrecision keep trailing zeros (fixed form) ----
console.log((1.5).toPrecision(3));   // "1.50"   was "1.5"
console.log((0).toPrecision(3));     // "0.00"   was "0"
console.log((0).toPrecision(6));     // "0.00000" was "0"
console.log((1.5).toPrecision(6));   // "1.50000"

// ---- toPrecision keep trailing zeros (exp form) ----
console.log((100).toPrecision(1));   // "1e+2"
console.log((100).toPrecision(2));   // "1.0e+2"  was "1e+2"
console.log((100).toPrecision(3));   // "100"
console.log((1e-7).toPrecision(2));  // "1.0e-7"  was "1e-7"
console.log((1234567).toPrecision(3));// "1.23e+6"

// ---- toExponential regression coverage ----
console.log((1).toExponential());     // "1e+0"
console.log((1e21).toExponential(2));  // "1.00e+21"
console.log((0).toExponential(5));     // "0.00000e+0"
