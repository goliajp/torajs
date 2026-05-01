// Integration: bitwise operators at characteristic boundaries.
// Exercises i64 truncation behavior, shifts, masks, and the
// interaction between Math.imul / Math.clz32 and bitwise ops.
function check(): number {
  // OR / AND / XOR identities.
  if ((0xff & 0xf0) !== 0xf0) { throw "#1: AND"; }
  if ((0xf0 | 0x0f) !== 0xff) { throw "#2: OR"; }
  if ((0xff ^ 0xf0) !== 0x0f) { throw "#3: XOR"; }
  if ((~0) !== -1) { throw "#4: bitwise not"; }
  if ((~(-1)) !== 0) { throw "#5"; }

  // Shifts.
  if ((1 << 4) !== 16) { throw "#6: shl"; }
  if ((256 >> 2) !== 64) { throw "#7: shr"; }
  if ((-1 >> 0) !== -1) { throw "#8: shr by zero"; }

  // Mask + shift compose (extracting nibbles).
  let n = 0xabcd;
  let lo = n & 0x0f;
  let hi = (n >> 4) & 0x0f;
  if (lo !== 0xd) { throw "#9"; }
  if (hi !== 0xc) { throw "#10"; }

  // Power-of-two check: (n & (n - 1)) == 0 iff n is power of 2.
  if ((8 & (8 - 1)) !== 0) { throw "#11: 8 is pow2"; }
  if ((6 & (6 - 1)) === 0) { throw "#12: 6 is not pow2"; }

  // Math.clz32 + 1 << bit: extract MSB.
  let v = 0x40;
  let bit = 31 - Math.clz32(v);
  if (bit !== 6) { throw "#13"; }
  if ((1 << bit) !== v) { throw "#14: round-trip"; }

  // Math.imul as 32-bit mul.
  if (Math.imul(0x10000, 0x10000) !== 0) { throw "#15: overflow truncates"; }
  if (Math.imul(7, 11) !== 77) { throw "#16"; }
  return 0;
}
console.log(check());
