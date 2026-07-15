// Number.prototype.toExponential / toPrecision spec-order:
// non-finite `x` short-circuits BEFORE the `f`/`p` range check.
// - ES §21.1.3.3 (toExponential): step 4 "If x is not finite, return
//   Number::toString(x, 10)" precedes step 5 "If f < 0 or f > 100
//   throw RangeError".
// - ES §21.1.3.5 (toPrecision): step 4 "If x is not finite, return
//   Number::toString(x, 10)" precedes step 5 "If p < 1 or p > 100
//   throw RangeError".
// Pre-fix tr checked digits/precision first, so
// `NaN.toExponential(Infinity)` / `NaN.toPrecision(1000)` /
// `(±Infinity).to{Exponential,Precision}(1000)` all threw
// RangeError instead of returning the special-value string.
// test262 cluster: Number/prototype/toExponential/{nan,infinity}.js
// + Number/prototype/toPrecision/{nan,infinity}.js.

// toExponential — non-finite x, out-of-range f
console.log(NaN.toExponential(Infinity));
console.log((+Infinity).toExponential(1000));
console.log((-Infinity).toExponential(1000));
console.log(NaN.toExponential(-1));
console.log((+Infinity).toExponential(-5));

// toPrecision — non-finite x, out-of-range p
console.log(NaN.toPrecision(1000));
console.log((+Infinity).toPrecision(1000));
console.log((-Infinity).toPrecision(1000));
console.log(NaN.toPrecision(0));
console.log((+Infinity).toPrecision(0));

// Finite x with out-of-range digits — RangeError still thrown
// (regression sentinel for the range gate).
try {
  (1.5).toExponential(-1);
  console.log("neg-exp did not throw");
} catch (e) {
  console.log("neg-exp caught:", (e as Error).message);
}
try {
  (1.5).toExponential(200);
  console.log("big-exp did not throw");
} catch (e) {
  console.log("big-exp caught:", (e as Error).message);
}
try {
  (1.5).toPrecision(0);
  console.log("zero-prec did not throw");
} catch (e) {
  console.log("zero-prec caught:", (e as Error).message);
}
try {
  (1.5).toPrecision(200);
  console.log("big-prec did not throw");
} catch (e) {
  console.log("big-prec caught:", (e as Error).message);
}

// Finite baseline — still routes through the normal formatter.
console.log((1.5).toExponential(2));
console.log((1234).toPrecision(4));
