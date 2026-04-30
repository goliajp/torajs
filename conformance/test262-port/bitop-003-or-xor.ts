// Adapted from test262: bitwise-or + bitwise-xor S11.10.* —
// pairwise checks on canonical inputs.
function check(): number {
  if ((0x0F | 0xF0) !== 0xFF) { throw "#1"; }
  if ((0xFF | 0xFF) !== 0xFF) { throw "#2"; }
  if ((0 | 42) !== 42) { throw "#3"; }
  if ((0xFF ^ 0x0F) !== 0xF0) { throw "#4"; }
  if ((0xAA ^ 0x55) !== 0xFF) { throw "#5"; }
  if ((42 ^ 42) !== 0) { throw "#6"; }
  return 0;
}
console.log(check());
