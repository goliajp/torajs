// Adapted from test262: language/expressions/left-shift +
// right-shift (signed) — int-range only to dodge int32-coercion edge cases.
function check(): number {
  if ((1 << 0) !== 1) { throw "#1"; }
  if ((1 << 4) !== 16) { throw "#2"; }
  if ((1 << 30) !== 1073741824) { throw "#3"; }
  if ((-1 << 1) !== -2) { throw "#4"; }
  if ((128 >> 0) !== 128) { throw "#5"; }
  if ((128 >> 4) !== 8) { throw "#6"; }
  if ((-1 >> 0) !== -1) { throw "#7"; }
  if ((-128 >> 1) !== -64) { throw "#8"; }
  return 0;
}
console.log(check());
