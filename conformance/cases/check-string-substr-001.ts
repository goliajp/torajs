// T-49 — String.prototype.substr (annexB legacy §B.2.3.1). tora had
// slice / substring but the 2nd-arg-is-length variant `substr` was
// missing — ssa_lower's String-method dispatch table didn't list it
// in the outer matches! gate, so `'abc'.substr(...)` fell through to
// the catch-all `unsupported member call shape: substr` panic.
// Unblocks 1 case under annexB/built-ins/String/prototype/substr
// (visible in the 5k sample; more substr-suite cases live outside
// the sample window).
//
// Spec corners exercised:
//   - negative start wraps to max(size+start, 0) — distinct from
//     substring's clamp-to-0 behavior
//   - 1-arg form: length defaults to "remaining" (i64::MAX sentinel
//     passed from SSA, runtime clamps to size-start)
//   - 2-arg form: explicit length
//   - fractional start: ToInteger truncates toward zero
//   - out-of-range start: returns empty string

console.log('abc'.substr(-1));    // 'c'
console.log('abc'.substr(-2));    // 'bc'
console.log('abc'.substr(-3));    // 'abc'
console.log('abc'.substr(-4));    // 'abc' (size + start < 0 → 0)
console.log('abc'.substr(-1.1));  // 'c'  (rounding)

console.log('abc'.substr(0));     // 'abc'
console.log('abc'.substr(1));     // 'bc'
console.log('abc'.substr(10));    // ''   (start > size → empty)

console.log('abc'.substr(0, 2));  // 'ab'
console.log('abc'.substr(1, 1));  // 'b'
console.log('abc'.substr(-2, 1)); // 'b'
console.log('abc'.substr(0, 99)); // 'abc' (length > avail → clamp)
console.log('abc'.substr(0, 0));  // ''
console.log('abc'.substr(0, -1)); // '' (length < 0 → clamp 0)
