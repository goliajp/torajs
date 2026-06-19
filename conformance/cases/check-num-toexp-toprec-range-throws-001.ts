// ES §22.1.3.5 step 3 / §22.1.3.32 step 3 — `toExponential(digits)`
// requires `digits` in `[0, 100]`; `toPrecision(precision)` requires
// `precision` in `[1, 100]`. Out-of-range throws RangeError. Pre-fix
// tr silently clamped both via `.clamp(0, 100)` / `.clamp(1, 100)`
// and returned a formatted string instead of throwing.

// toExponential negative.
let caught_exp_neg = false;
let exp_neg_msg = "";
try {
  (1.5).toExponential(-1);
} catch (e: any) {
  caught_exp_neg = true;
  exp_neg_msg = (e as Error).message;
}
console.log("toExp(-1) threw:", caught_exp_neg);
console.log("toExp(-1) msg:", exp_neg_msg);

// toExponential 101.
let caught_exp_big = false;
try {
  (1.5).toExponential(101);
} catch (e: any) {
  caught_exp_big = true;
}
console.log("toExp(101) threw:", caught_exp_big);

// toExponential(100) is the inclusive upper bound — no throw.
const big = (1).toExponential(100);
console.log("toExp(100) prefix:", big.slice(0, 3));
console.log("toExp(100) tail:", big.slice(big.length - 3));

// Common toExponential unchanged.
console.log((1234.5).toExponential(2));
console.log((0.0007).toExponential(3));

// Integer receiver toExponential — same gate.
let caught_exp_int = false;
try {
  (42).toExponential(-1);
} catch (e: any) {
  caught_exp_int = true;
}
console.log("toExp int(-1) threw:", caught_exp_int);

// No-arg toExponential keeps the shortest-form path (the SSA
// passes `-1` sentinel, carved out of the gate).
console.log((1234567890).toExponential());

// toPrecision below 1.
let caught_prec_zero = false;
let prec_zero_msg = "";
try {
  (1.5).toPrecision(0);
} catch (e: any) {
  caught_prec_zero = true;
  prec_zero_msg = (e as Error).message;
}
console.log("toPrec(0) threw:", caught_prec_zero);
console.log("toPrec(0) msg:", prec_zero_msg);

// toPrecision 101.
let caught_prec_big = false;
try {
  (1.5).toPrecision(101);
} catch (e: any) {
  caught_prec_big = true;
}
console.log("toPrec(101) threw:", caught_prec_big);

// toPrecision 1 inclusive.
console.log((1.234).toPrecision(1));
console.log((1.234).toPrecision(100).slice(0, 6));

// Integer receiver toPrecision — same gate.
let caught_prec_int = false;
try {
  (42).toPrecision(0);
} catch (e: any) {
  caught_prec_int = true;
}
console.log("toPrec int(0) threw:", caught_prec_int);

// No-arg toPrecision still routes to Number.toString (SSA-level
// early return, doesn't hit the precision_f wrapper).
console.log((1.234).toPrecision());

// After-catch sanity.
try {
  (1.5).toExponential(-1);
} catch (e: any) {}
console.log("after-catch:", (1.5).toExponential(2), (1.5).toPrecision(3));
