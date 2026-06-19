// `arr.with(index, value)` per ES §23.1.3.39 step 7 must throw
// RangeError when actualIndex is out of [0, len). Pre-fix
// `__torajs_arr_with` skipped the bounds check, fell through to
// `*(dst + adj*8) = v`, and silently corrupted adjacent heap (or
// the alloc's slack on small caps) — `[1,2,3].with(99, 0)` printed
// `[ 1, 2, 3 ]` (the clone) instead of throwing. Fix adds the
// spec-shaped `adj < 0 || adj >= len` guard with a TLS
// `__torajs_throw_range_error`; ssa_lower's with-call site emits
// `emit_throw_check` after the call so the throw lands at the
// caller's nearest `try/catch` (and the rc-inc walk never runs
// against the null returned on error).

console.log([1, 2, 3].with(0, 99));
console.log([1, 2, 3].with(2, 99));
console.log([1, 2, 3].with(-1, 99));
console.log([1, 2, 3].with(-3, 99));

try {
  console.log([1, 2, 3].with(3, 0));
} catch (e: any) {
  console.log('caught: ' + e.message);
}
try {
  console.log([1, 2, 3].with(99, 0));
} catch (e: any) {
  console.log('caught: ' + e.message);
}
try {
  console.log([1, 2, 3].with(-4, 0));
} catch (e: any) {
  console.log('caught: ' + e.message);
}
try {
  console.log([1, 2, 3].with(-99, 0));
} catch (e: any) {
  console.log('caught: ' + e.message);
}

// Empty array — every index is OOB.
try {
  console.log([].with(0, 0));
} catch (e: any) {
  console.log('caught: ' + e.message);
}
