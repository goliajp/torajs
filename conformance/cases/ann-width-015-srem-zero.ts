// W3 §5.3 follow-up close — srem runtime-0 divisor. The int-path
// carve (non-negative constant dividend) and the frem_narrow srem
// recovery both need a PROVABLY non-zero divisor: aarch64
// sdiv-by-zero yields 0 and the msub hands the dividend back, so
// `7 % b` with b == 0 silently printed 7 where the spec answer is
// NaN. Live silent-wrong on plain shapes, independent of S9.

// carve face: const dividend, runtime-zero variable divisor.
function modc(b: number): number {
  return 7 % b;
}
console.log(modc(0));  // NaN
console.log(modc(4));  // 3

// top-level variable divisor.
let z = 0;
console.log(7 % z);  // NaN

// frem_narrow recovery face: the i64 sink must keep the frem when
// the divisor is a variable (NaN → fptosi → 0).
function modOr0(b: number): number {
  return (7 % b) | 0;
}
console.log(modOr0(0));  // 0
console.log(modOr0(4));  // 3

// compare sink: NaN === 0 is false (an srem recovery would compute
// 0 % 0 == 0 and wrongly take the branch).
function isDivisible(a: number, b: number): boolean {
  return a % b === 0;
}
console.log(isDivisible(0, 0));  // false
console.log(isDivisible(8, 4));  // true

// provable faces keep the int path end to end.
function wrap(k: number): number {
  return k + 100 % 7;
}
console.log(wrap(1));  // 3
console.log(12 % 5);  // 2
