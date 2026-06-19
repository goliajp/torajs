// ES §22.1.3.32 step 3 — `n.toFixed(digits)` throws RangeError when
// `digits` is outside `[0, 100]`. Pre-fix tr's helper silently
// clamped via `digits.clamp(0, 20)` (carry-over from the older
// pre-ES2020 spec), so negative / out-of-range args returned a
// formatted string instead of throwing.

// Negative argument throws.
let caught_neg = false;
let neg_msg = "";
try {
  (1.5).toFixed(-1);
} catch (e: any) {
  caught_neg = true;
  neg_msg = (e as Error).message;
}
console.log("(-1) threw:", caught_neg);
console.log("(-1) msg:", neg_msg);

// 101 throws.
let caught_big = false;
try {
  (1.5).toFixed(101);
} catch (e: any) {
  caught_big = true;
}
console.log("(101) threw:", caught_big);

// 100 is the inclusive upper bound — no throw.
const big = (1).toFixed(100);
console.log("(100) len:", big.length);
console.log("(100) prefix:", big.slice(0, 3));

// 0 is valid.
console.log((3.7).toFixed(0));

// Common case unchanged.
console.log((1.5).toFixed(2));
console.log((1234567.89).toFixed(2));

// Integer receiver — same gate.
let caught_int = false;
try {
  (42).toFixed(-5);
} catch (e: any) {
  caught_int = true;
}
console.log("int(-5) threw:", caught_int);

// After-catch sanity — TLS pending throw cleared.
try {
  (1.5).toFixed(-1);
} catch (e: any) {}
console.log("after-catch:", (1.5).toFixed(3));
