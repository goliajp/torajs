// S170 — `Number.prototype.toLocaleString` tiny-magnitude collapse
// per ECMA-402 defaults `maximumFractionDigits = 3` +
// `minimumFractionDigits = 0`. Values with `|n| < 5e-4` round to a
// single zero digit (half-away-from-zero); sign-preserve so tiny
// negatives render as `"-0"` (bun's `(-1e-6).toLocaleString()` is
// `"-0"`, not `"-0.000001"`).

// 1) Pre-S170 wedge — tiny negative, sign-preserved.
console.log((-1e-6).toLocaleString());

// 2) Tiny positive — single "0".
console.log((1e-6).toLocaleString());
console.log((1e-7).toLocaleString());

// 3) 0.0001 — magnitude rounds to 0.
console.log((0.0001).toLocaleString());
console.log((-0.0001).toLocaleString());

// 4) Below-threshold boundary (just under 5e-4).
console.log((0.00049).toLocaleString());
console.log((-0.00049).toLocaleString());

// 5) Above-threshold rounds away from zero — still goes through the
// existing path (the narrow fix is scoped to `|n| < 5e-4`; broader
// half-away-from-zero rounding is L3b).
console.log((0.5).toLocaleString());
console.log((1.5).toLocaleString());

// 6) Integer/0 regression — must keep prior behavior.
console.log((0).toLocaleString());
console.log((1000).toLocaleString());
console.log((-1000).toLocaleString());
console.log((1000000).toLocaleString());
