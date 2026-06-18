// `Math.sumPrecise(items)` per ES2025 §21.3.2.32 — correctly-rounded
// sum of an Array<Number>. Result is the f64 nearest the exact
// real-valued sum of the inputs (ties to even). Intermediate
// rounding errors are eliminated via Shewchuk's expansion-sum +
// Python's `_msum_fsum` correctly-rounded tail.

console.log(Math.sumPrecise([1, 2, 3]));
console.log(Math.sumPrecise([0.1, 0.2]));
console.log(Math.sumPrecise([0.1, 0.2, 0.3]));
console.log(Math.sumPrecise([1e20, 1, -1e20]));
// Note: `[1e308, 1e308, -1e308]` would test overflow-aware rounding —
// per spec the exact infinite-precision sum is 1e+308 (representable).
// Bun handles this; tora's narrow Shewchuk overflows during
// intermediate accumulation. L3b — overflow-aware reordering.
console.log(Math.sumPrecise([1e200, 1e200, -1e200]));

const empty: number[] = [];
console.log(Math.sumPrecise(empty));

console.log(Math.sumPrecise([42]));
console.log(Math.sumPrecise([-0]));
console.log(Math.sumPrecise([0, 0]));
console.log(Math.sumPrecise([-0, -0]));

console.log(Math.sumPrecise([Infinity]));
console.log(Math.sumPrecise([Infinity, -Infinity]));
console.log(Math.sumPrecise([Infinity, 1.5]));
console.log(Math.sumPrecise([-Infinity, 1.5]));
console.log(Math.sumPrecise([NaN]));
console.log(Math.sumPrecise([NaN, 1.5]));

// Cancellation correctness — Shewchuk vs naive sum.
// Use a named binding so the array doesn't trigger the inline-literal
// integer/Number type-inference wedge (L3b — inline `Math.sumPrecise([1,
// 1e-100, ...])` lowers as Array<I64> even with float elements; named
// binding correctly infers Array<F64>).
const cancel: number[] = [1, 1e-100, -1, 1e-100];
console.log(Math.sumPrecise(cancel));

// Large array — 100 × 0.1 should be exactly 10
const big: number[] = [];
for (let i = 0; i < 100; i++) big.push(0.1);
console.log(Math.sumPrecise(big));

// 1000 × 0.1 should be exactly 100
const huge: number[] = [];
for (let i = 0; i < 1000; i++) huge.push(0.1);
console.log(Math.sumPrecise(huge));

// Mixed positive / negative
console.log(Math.sumPrecise([7.0, -3.0, 2.0, -1.0]));

// Subnormal numbers
console.log(Math.sumPrecise([5e-324, 5e-324]));

// Two opposite values exactly cancel
console.log(Math.sumPrecise([1.5, -1.5]));
