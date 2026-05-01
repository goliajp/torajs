// Integration: edge cases across Number methods, constants, and
// global predicates. Tests defaults, negative zero (where expressible),
// safe-integer boundaries, and the coercion path between i64 / f64.
function check(): number {
  // Safe integer boundaries.
  if (Number.isSafeInteger(Number.MAX_SAFE_INTEGER) !== true) { throw "#1"; }
  if (Number.isSafeInteger(Number.MIN_SAFE_INTEGER) !== true) { throw "#2"; }
  if (Number.isSafeInteger(Number.MAX_SAFE_INTEGER + 1) !== false) { throw "#3"; }

  // Trig identities.
  if (Math.sin(0) !== 0) { throw "#4"; }
  if (Math.cos(0) !== 1) { throw "#5"; }

  // parseInt with various radixes.
  if (parseInt("100", 2) !== 4) { throw "#6: binary"; }
  if (parseInt("100", 8) !== 64) { throw "#7: octal"; }
  if (parseInt("100", 10) !== 100) { throw "#8: dec"; }
  if (parseInt("100", 16) !== 256) { throw "#9: hex"; }

  // Min/Max chain.
  let xs: number[] = [3, 7, 1, 9, 4, 6];
  let min = xs[0];
  let max = xs[0];
  for (let v of xs) {
    if (v < min) { min = v; }
    if (v > max) { max = v; }
  }
  if (min !== 1) { throw "#10"; }
  if (max !== 9) { throw "#11"; }
  if (Math.min(3, 7, 1, 9, 4, 6) !== 1) { throw "#12: variadic min"; }
  if (Math.max(3, 7, 1, 9, 4, 6) !== 9) { throw "#13"; }

  // Math.abs across signed types.
  if (Math.abs(-7) !== 7) { throw "#14"; }
  if (Math.abs(7) !== 7) { throw "#15"; }
  if (Math.abs(0) !== 0) { throw "#16"; }

  // Floor / ceil / round consistency.
  if (Math.floor(3.7) !== 3) { throw "#17"; }
  if (Math.ceil(3.2) !== 4) { throw "#18"; }
  if (Math.round(3.5) !== 4) { throw "#19"; }
  if (Math.trunc(3.7) !== 3) { throw "#20"; }
  if (Math.trunc(-3.7) !== -3) { throw "#21: trunc toward zero"; }

  // Clamping via min/max — Math.min/max returns f64 so the lambda is
  // explicitly f64-typed (tr's `: number` slot defaults to i64).
  let clamp = (v: number, lo: number, hi: number): f64 =>
    Math.min(Math.max(v, lo), hi);
  if (clamp(50, 0, 100) !== 50) { throw "#22"; }
  if (clamp(-10, 0, 100) !== 0) { throw "#23"; }
  if (clamp(150, 0, 100) !== 100) { throw "#24"; }
  return 0;
}
console.log(check());
