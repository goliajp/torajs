// ES §22.1.3.16 — String.prototype.repeat(n)
//   step 4: If n < 0, throw a RangeError exception.
// Pre-fix tr silently clamped negative n to 0 (the C-port "v0
// subset" carry-over); now the runtime sets a TLS pending throw
// and ssa_lower's post-call emit_throw_check propagates to user
// try/catch.
//
// Note: the exact RangeError message differs between engines
// (V8/JSC ship a long template; tr uses a shorter "Invalid count
// value"); fixture only checks the throw was raised + caught,
// not the message text.
let caught_neg = false;
try {
  "a".repeat(-1);
} catch (e: any) {
  caught_neg = true;
}
console.log("repeat(-1) threw:", caught_neg);

let caught_neg_long = false;
try {
  "abc".repeat(-100);
} catch (e: any) {
  caught_neg_long = true;
}
console.log("repeat(-100) threw:", caught_neg_long);

// Non-throwing baseline cases — sanity regression
console.log("repeat(0):", "a".repeat(0));      // ""
console.log("repeat(3):", "a".repeat(3));      // "aaa"
console.log("repeat(2):", "ab".repeat(2));     // "abab"
console.log("repeat(5)e:", "".repeat(5));      // ""

// Smoke that a normal call after a caught throw works (no TLS
// residue blocking subsequent repeats)
try {
  "x".repeat(-1);
} catch (e: any) {}
console.log("after-catch:", "x".repeat(4));    // "xxxx"
