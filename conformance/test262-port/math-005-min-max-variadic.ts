// Adapted from test262: built-ins/Math/{min,max}/* — JS spec accepts
// any number of args (>=0; subset requires >=2). tr previously only
// accepted exactly 2; the typecheck arm now special-cases Math.min /
// Math.max for variadic calls and ssa-lower folds the call into a
// pairwise reduction (left-to-right: min(min(a,b),c), …).
function check(): number {
  // Two-arg — unchanged.
  if (Math.min(3, 7) !== 3) { throw "#1"; }
  if (Math.max(3, 7) !== 7) { throw "#2"; }

  // Three-arg — first multi-fold.
  if (Math.min(3, 7, 1) !== 1) { throw "#3"; }
  if (Math.max(3, 7, 1) !== 7) { throw "#4"; }

  // Four-arg.
  if (Math.min(3, 7, 1, 9) !== 1) { throw "#5"; }
  if (Math.max(3, 7, 1, 9) !== 9) { throw "#6"; }

  // Five-arg with negative values.
  if (Math.min(-1, -5, 3, -3, 0) !== -5) { throw "#7"; }
  if (Math.max(-1, -5, 3, -3, 0) !== 3) { throw "#8"; }

  // Eight-arg — long fold.
  if (Math.min(8, 4, 2, 6, 1, 9, 5, 3) !== 1) { throw "#9"; }
  if (Math.max(8, 4, 2, 6, 1, 9, 5, 3) !== 9) { throw "#10"; }

  // Mixed integer + (already-integer-valued) literal.
  if (Math.min(2.5, 1.5, 0.5) !== 0.5) { throw "#11"; }
  return 0;
}
console.log(check());
