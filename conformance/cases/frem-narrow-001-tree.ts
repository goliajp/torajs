// W3 chunk 2 (rfc 20260611 §5.3 follow-up) — truncation propagation
// through f-op trees: fptosi over a whole fadd/fsub/fmul/frem tree
// with int leaves retypes to int ops (V8 Truncation::Word64 shape).
// Negative dividends keep the C1 float `%` lowering, so each tree
// exercises the recovery rather than the never-floated int path.
function t1(a: number, b: number): number {
  return ((a % 3) + (b % 2)) | 0;
}
function t2(a: number, b: number, c: number): number {
  return ((a % 5) * (b % 7) - (c % 3)) | 0;
}
function t3(a: number, b: number): number {
  return ((a % 4) * (a % 4) + (b % 9) * (b % 9)) | 0;
}
console.log(t1(7, 5), t1(-7, 5), t1(7, -5));
console.log(t2(9, 8, 7), t2(-9, 8, -7), t2(9, -8, 7));
console.log(t3(6, 10), t3(-6, -10));
