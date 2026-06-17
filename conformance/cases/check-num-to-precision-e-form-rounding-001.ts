// ES §22.1.3.36 — Number.prototype.toPrecision %e-form ties round
// half-away-from-zero (follow-up to `a5255701` which fixed %f form).
// Routes through %e form when `x < -4 || x >= precision`. Rust
// `format!("{:.*e}", _, _)` rounds half-to-even (banker), so a tie at
// the last kept mantissa digit with an even preceding digit went down
// instead of away from zero.
console.log((125).toPrecision(2));         // 1.25e+2 tie → "1.3e+2"
console.log((-125).toPrecision(2));        // -1.25e+2 tie → "-1.3e+2"
console.log((1245).toPrecision(3));        // 1.245e+3 tie → "1.25e+3"
console.log((-1245).toPrecision(3));       // -1.245e+3 tie → "-1.25e+3"
console.log((12450).toPrecision(4));       // 1.2450e+4 (no tie at 4th frac)
console.log((0.000125).toPrecision(2));    // 1.25e-4 tie → "1.3e-4"
console.log((1.5e21).toPrecision(2));      // very large — spec defers to ToString
// non-tie %e-form smoke (guard the fix didn't change non-tie cases)
console.log((1235).toPrecision(2));        // 1.235e+3 not a tie at 1 frac → "1.2e+3"
console.log((1255).toPrecision(2));        // 1.255e+3 not a tie at 1 frac → "1.3e+3"
// also smoke toExponential — same Rust banker leak (no follow-up commit
// has fixed this yet)
console.log((1.25).toExponential(1));      // tie → "1.3e+0"
console.log((-1.25).toExponential(1));     // tie → "-1.3e+0"
console.log((1.245).toExponential(2));     // tie → "1.25e+0"
