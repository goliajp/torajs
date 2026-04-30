// Adapted from test262: language/expressions/bitwise-and/S11.10.1_A2.1.js
// Bitwise AND on integer-shape doubles → JS spec converts to int32, ANDs.
// All test inputs fit in int32 to dodge that subtlety.
function check(): number {
  if ((0xFF & 0x0F) !== 0x0F) { throw "#1"; }
  if ((0xF0 & 0x0F) !== 0x00) { throw "#2"; }
  if ((0xAA & 0x55) !== 0x00) { throw "#3"; }
  if ((-1 & 0x0F) !== 0x0F) { throw "#4"; }
  if ((0 & 12345) !== 0) { throw "#5"; }
  return 0;
}
console.log(check());
