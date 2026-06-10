// interval range analysis seeding sext_elide — loop-carried cells the
// one-pass lattice can't certify get their sext pairs elided when the
// branch-refined interval proves them in-i32-range. ToInt32 semantics
// must hold both when the seed fires (bounded counter loops) and when
// it must NOT fire (cells that genuinely leave i32 range and need the
// wrap).

// bounded counter loop — the inline-popcount shape: the n cell is
// provably inside [0, 1e5), every sext pair in the hot loop elides.
let total: number = 0;
for (let i = 0; i < 100000; i++) {
  let n: number = i;
  let bits: number = 0;
  while (n !== 0) {
    n = n & (n - 1);
    bits = bits + 1;
  }
  total = total + bits;
}
console.log(total);

// cell leaves i32 range — the interval must NOT certify it, and the
// ToInt32 wrap on the bitwise op must survive. 3000000000 > INT32_MAX
// so (j | 0) wraps to a negative int32.
let j: number = 0;
let acc: number = 0;
while (j <= 3000000000) {
  acc = acc ^ (j | 0);
  j = j + 750000000;
}
console.log(acc);

// negative-running cell inside i32 range — refinement on a sgt-shaped
// bound, pairs elide on the negative side too.
let k: number = 0;
let sum: number = 0;
while (k > -50000) {
  sum = sum + (k & 1023);
  k = k - 7;
}
console.log(sum);

// cell bounded only by an equality-free condition the refinement
// can't represent (k2 != sentinel) — stays unseeded, wrap preserved.
let k2: number = 2147483640;
let out: number = 0;
let steps: number = 0;
while (steps !== 4) {
  out = out ^ (k2 & -1);
  k2 = k2 + 3;
  steps = steps + 1;
}
console.log(out);
