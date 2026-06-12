// W3 S9 (rfc 20260611 §5.3 follow-up) — var×var int Mul preserves
// IEEE -0: runtime zero × runtime negative must produce -0, so the
// product lowers through the float path (width_of mirrors f64); the
// chunk-2 trunc-tree recovery keeps i64-sink shapes on the int path.
// The float product also aligns past-2^53 rounding with JS Number.
function m(a: number, b: number): number {
  return a * b;
}
console.log(1 / m(0, -1));
console.log(1 / m(-1, 0));
console.log(1 / m(0, 1));
console.log(m(6, 7));
console.log(Object.is(m(0, -1), -0));
console.log(m(123456789, 987654321));
function idx(a: number, b: number, c: number): number {
  return (a * b + c) | 0;
}
console.log(idx(3, 4, 5), idx(-3, 4, 5), idx(0, -7, 2));
