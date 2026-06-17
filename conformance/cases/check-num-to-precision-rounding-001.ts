// ES §22.1.3.36 — `Number.prototype.toPrecision` ties go to the
// larger m (round half away from zero). Rust's
// `format!("{:.*}", _, _)` rounds half-to-even (banker's), same
// wedge `to_fixed_f` already neutralizes via explicit pre-rounding.
// Pre-fix `(1234.5).toPrecision(4)` was "1234"; spec / bun "1235".

console.log((1234.5).toPrecision(4))   // "1235"
console.log((-1234.5).toPrecision(4))  // "-1235"
console.log((2.5).toPrecision(1))      // "3"
console.log((1.5).toPrecision(1))      // "2"
console.log((0.5).toPrecision(1))      // "0.5" (X=-1, %f, frac=1 no round)
console.log((1234.6).toPrecision(4))   // "1235" (non-tie, banker == away)
console.log((1234.4).toPrecision(4))   // "1234" (round down, both)

// Regression guards — already covered by existing
// number-tofixed-toprecision-spec-001 but spotted here to keep
// the rounding-focused fixture self-contained:
console.log((1.5).toPrecision(3))      // "1.50"
console.log((0).toPrecision(3))        // "0.00"
console.log((100).toPrecision(3))      // "100"
console.log((1234567).toPrecision(3))  // "1.23e+6"
