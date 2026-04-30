// Adapted from test262: language/expressions/bitwise-and/S11.10.1_A4_T1.js + siblings.
// Tests bitwise & | ^ << >> on integer values.
function check(): number {
  if ((0xFF & 0x0F) !== 0x0F) { throw "#1"; }
  if ((0x10 | 0x01) !== 0x11) { throw "#2"; }
  if ((0xAA ^ 0xFF) !== 0x55) { throw "#3"; }
  if ((1 << 4) !== 16) { throw "#4"; }
  if ((64 >> 2) !== 16) { throw "#5"; }
  if ((0 & 1) !== 0) { throw "#6"; }
  if ((1 | 0) !== 1) { throw "#7"; }
  if ((1 ^ 1) !== 0) { throw "#8"; }
  return 0;
}
console.log(check());
