// ES §21.1.3.6 step 4 — `n.toString(radix)` throws RangeError when
// `radix` is outside `[2, 36]`. Pre-fix tr's helpers silently
// clamped via `.clamp(2, 36)` so `(10).toString(1)` returned
// `"10"` instead of throwing.

// i64 receiver — below 2 throws.
let caught_i_lo = false;
let i_lo_msg = "";
try {
  (10).toString(1);
} catch (e: any) {
  caught_i_lo = true;
  i_lo_msg = (e as Error).message;
}
console.log("int(1) threw:", caught_i_lo);
console.log("int(1) msg:", i_lo_msg);

// i64 receiver — above 36 throws.
let caught_i_hi = false;
try {
  (10).toString(37);
} catch (e: any) {
  caught_i_hi = true;
}
console.log("int(37) threw:", caught_i_hi);

// f64 receiver — same gate.
let caught_f_lo = false;
try {
  (1.5).toString(0);
} catch (e: any) {
  caught_f_lo = true;
}
console.log("f64(0) threw:", caught_f_lo);

let caught_f_hi = false;
try {
  (1.5).toString(100);
} catch (e: any) {
  caught_f_hi = true;
}
console.log("f64(100) threw:", caught_f_hi);

// Inclusive bounds 2 and 36 — no throw.
console.log((10).toString(2));
console.log((10).toString(36));
console.log((255).toString(16));
console.log((1.5).toString(2));

// No-arg toString unchanged.
console.log((10).toString());
console.log((1.5).toString());
console.log((-1).toString());

// After-catch sanity.
try {
  (10).toString(1);
} catch (e: any) {}
console.log("after-catch:", (10).toString(8));
