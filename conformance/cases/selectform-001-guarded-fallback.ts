// select-formation guard side-exit semantics (RFC 20260719 blade 3):
// collatz steps from a start big enough that 3n+1 crosses the
// float_demote window guard (n > ~3.0e15) mid-trajectory, forcing the
// speculated guard reduction to fire and fall back to the f64 region.
// The demoted int loop and the fallback must agree with bun exactly.
function steps(n: number): number {
  let count: number = 0;
  while (n !== 1) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = 3 * n + 1;
    }
    count = count + 1;
  }
  return count;
}

// 2^51 + 1: odd, so the first step computes 3n+1 ≈ 6.7e15 — above the
// window guard (3002399751580330 ≈ 2^51.4) yet exactly representable
// in f64 (< 2^53), so the fallback stays exact.
console.log(steps(2251799813685249));
// small controls straddling the guard-free path
console.log(steps(27));
console.log(steps(1));
