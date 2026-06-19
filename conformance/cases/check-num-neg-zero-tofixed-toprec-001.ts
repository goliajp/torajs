// ES §21.1.3.3 / §21.1.3.5: (-0).toFixed(N) and (-0).toPrecision(N)
// emit a positive-sign result because step 7 of toFixed (and the
// equivalent in toPrecision) tests `x < 0`, and IEEE 754 `-0` is
// equal to `0` so the test is false. Rust's `format!("{:.N}", -0.0)`
// preserves the sign bit and would emit "-0.00" — to_exp_f already
// normalises -0 → +0 at entry (line 157); to_fixed_f and
// to_precision_f now mirror that wedge.

console.log((-0).toFixed(2));
console.log((-0).toFixed(0));
console.log((-0).toFixed(5));
console.log((-0).toPrecision(3));
console.log((-0).toPrecision(1));
console.log((-0).toPrecision(6));

// Positive 0 path unchanged.
console.log((0).toFixed(2));
console.log((0).toPrecision(3));

// Non-zero negatives unchanged — sign preserved.
console.log((-1).toFixed(2));
console.log((-1.5).toFixed(0));
console.log((-1.5).toPrecision(3));

// Truncated-to-zero is NOT exact -0; sign preserved per Rust
// formatter + spec (matches bun).
console.log((-0.0001).toFixed(2));
console.log((-0.0001).toPrecision(3));

// toExponential already normalises -0; regression check.
console.log((-0).toExponential(2));
console.log((-0).toExponential());
