// S279 — global isNaN / isFinite(undefined) per ES §19.2.{3,4}:
// ToNumber(undefined) = NaN, so isNaN→true, isFinite→false. Pre-S279
// tora silently dispatched the Undefined sentinel (i64 0) through
// the I64 arm of the num_is_nan_i / num_is_finite_i helpers,
// returning the *opposite* (false / true) — same shape as isNaN(0).

// undefined → ToNumber → NaN
console.log(isNaN(undefined));   // bun: true
console.log(isFinite(undefined)); // bun: false

// Regression panel — non-undef paths stay green
console.log(isNaN(NaN));        // bun: true
console.log(isFinite(NaN));     // bun: false
console.log(isNaN(0));          // bun: false
console.log(isFinite(0));       // bun: true
console.log(isNaN(Infinity));   // bun: false
console.log(isFinite(Infinity)); // bun: false
console.log(isNaN(3.14));       // bun: false
console.log(isFinite(3.14));    // bun: true

// Number.{isNaN,isFinite} — strict (no ToNumber coercion). undefined
// is not a Number, so both stay false.
console.log(Number.isNaN(undefined));    // bun: false
console.log(Number.isFinite(undefined)); // bun: false
