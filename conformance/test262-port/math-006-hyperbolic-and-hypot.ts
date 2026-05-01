// Adapted from test262: built-ins/Math/{sinh,cosh,tanh,asinh,acosh,atanh,
// expm1,log1p,hypot}/* — eight more libm-backed Math statics plus the
// variadic hypot. hypot is folded inline as sqrt(sum of args²) in
// ssa-lower (FMul + FAdd chain → math_sqrt).
function check(): number {
  // Hyperbolic at zero — exact.
  if (Math.sinh(0) !== 0) { throw "#1"; }
  if (Math.cosh(0) !== 1) { throw "#2"; }
  if (Math.tanh(0) !== 0) { throw "#3"; }
  if (Math.asinh(0) !== 0) { throw "#4"; }
  if (Math.atanh(0) !== 0) { throw "#5"; }
  if (Math.acosh(1) !== 0) { throw "#6: acosh(1)"; }

  // expm1(0) = 0, log1p(0) = 0 — exact.
  if (Math.expm1(0) !== 0) { throw "#7"; }
  if (Math.log1p(0) !== 0) { throw "#8"; }
  // expm1(1) ≈ E - 1 (bit-pattern diff between expm1 and exp-1 is OK).
  let e1_6 = Math.round(Math.expm1(1) * 1e6);
  let exp1_minus_1_6 = Math.round((Math.E - 1) * 1e6);
  if (e1_6 !== exp1_minus_1_6) { throw "#9: expm1 identity"; }

  // hypot — Pythagorean triples (exact).
  if (Math.hypot(3, 4) !== 5) { throw "#10: 3-4-5"; }
  if (Math.hypot(5, 12) !== 13) { throw "#11: 5-12-13"; }
  if (Math.hypot(8, 15) !== 17) { throw "#12: 8-15-17"; }

  // hypot — variadic (>=2 args).
  if (Math.hypot(1, 2, 2) !== 3) { throw "#13: 1²+2²+2²=9"; }
  if (Math.hypot(2, 3, 6) !== 7) { throw "#14: 2²+3²+6²=49"; }

  // hypot — single arg = abs(x).
  if (Math.hypot(5) !== 5) { throw "#15"; }
  if (Math.hypot(-7) !== 7) { throw "#16: |-7| = 7"; }

  // tanh asymptotes — round to 6 decimals.
  let large = Math.round(Math.tanh(20) * 1e6);
  if (large !== 1000000) { throw "#17: tanh(20) → 1"; }
  let neg_large = Math.round(Math.tanh(-20) * 1e6);
  if (neg_large !== -1000000) { throw "#18: tanh(-20) → -1"; }
  return 0;
}
console.log(check());
