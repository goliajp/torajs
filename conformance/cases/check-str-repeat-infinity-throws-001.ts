// ES §22.1.3.16 step 4 — `s.repeat(+Infinity)` throws RangeError.
// Pre-fix tr's SSA path lowered the f64 `Infinity` literal via
// `FpToSi` so it reached the runtime helper as `i64::MAX`, which
// passed the `n < 0` gate and entered the alloc loop with a u32
// wrapping-multiplied size — observably a silent no-op (empty
// string) rather than a throw. The fix is a 1-line helper check
// (`n == i64::MAX` is the +Infinity sentinel on aarch64 / x86_64
// FpToSi).
//
// Sanity-checks the throw still propagates via try/catch + the
// message matches the bun spec text so conformance diff-tests can
// strict-match.

let caught_inf = false;
let inf_msg = "";
try {
  "ab".repeat(Infinity);
} catch (e: any) {
  caught_inf = true;
  inf_msg = (e as Error).message;
}
console.log("repeat(Infinity) threw:", caught_inf);
console.log("repeat(Infinity) msg:", inf_msg);

// Negative case shares the new message — verify alignment.
let caught_neg = false;
let neg_msg = "";
try {
  "x".repeat(-1);
} catch (e: any) {
  caught_neg = true;
  neg_msg = (e as Error).message;
}
console.log("repeat(-1) threw:", caught_neg);
console.log("repeat(-1) msg:", neg_msg);

// NaN ToInteger = 0 per ES §7.1.4 — no throw, empty string.
console.log("repeat(NaN):", "ab".repeat(NaN));

// Sanity that the happy path still works after a thrown case.
try {
  "y".repeat(Infinity);
} catch (e: any) {}
console.log("after-catch:", "y".repeat(3));

// `-Infinity` already gets thrown by the existing n < 0 path
// (`FpToSi -Inf` is `i64::MIN`).
let caught_neg_inf = false;
try {
  "z".repeat(-Infinity);
} catch (e: any) {
  caught_neg_inf = true;
}
console.log("repeat(-Infinity) threw:", caught_neg_inf);
