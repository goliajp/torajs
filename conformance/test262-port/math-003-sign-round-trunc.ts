// Adapted from test262: built-ins/Math/{sign,round,trunc}/* —
// `Math.sign` returns -1/0/+1 (preserving zero's sign per JS spec);
// `Math.round` rounds half-values toward +∞ (so -2.5 → -2, NOT -3);
// `Math.trunc` chops toward zero. tr's runtime_str.c defines `sign`
// and `round` directly (libc has no sign and libc round disagrees on
// halves), `trunc` routes through libc.
function check(): number {
  if (Math.sign(5) !== 1) { throw "#1"; }
  if (Math.sign(-7) !== -1) { throw "#2"; }
  if (Math.sign(0) !== 0) { throw "#3"; }
  if (Math.sign(0.0001) !== 1) { throw "#4"; }

  if (Math.round(3.4) !== 3) { throw "#5"; }
  if (Math.round(3.6) !== 4) { throw "#6"; }
  if (Math.round(2.5) !== 3) { throw "#7: half toward +inf"; }
  if (Math.round(-2.5) !== -2) { throw "#8: -half toward +inf"; }
  if (Math.round(0) !== 0) { throw "#9"; }

  if (Math.trunc(3.7) !== 3) { throw "#10"; }
  if (Math.trunc(-3.7) !== -3) { throw "#11"; }
  if (Math.trunc(0) !== 0) { throw "#12"; }
  if (Math.trunc(2.99999) !== 2) { throw "#13"; }
  return 0;
}
console.log(check());
