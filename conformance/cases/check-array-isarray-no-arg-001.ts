// S204 — `Array.isArray()` accepts 0-arg per ES §23.1.2.2 step 1:
// missing arg defaults to undefined, undefined is not an Array,
// so the predicate is statically false.
// Pre-S204 tr panicked "index out of bounds" in ssa_lower because
// the Array.isArray match block hit `args[0]` directly without an
// arity short-circuit, and check.rs rejected the 0-arg call at the
// generic arity gate ("expected 1 argument(s), got 0").

console.log('isArray-no-arg:', Array.isArray());

// 1-arg regression — confirm the predicate's truth table is intact.
console.log('isArray-array:', Array.isArray([1, 2, 3]));
console.log('isArray-empty-array:', Array.isArray([]));
console.log('isArray-string:', Array.isArray('not an array'));
console.log('isArray-number:', Array.isArray(42));
console.log('isArray-object:', Array.isArray({}));
console.log('isArray-null:', Array.isArray(null));

// Boolean-cast in conditional contexts (typed downstream tests
// frequently feed Array.isArray result into an if-guard).
const xs: number[] = [1, 2];
if (Array.isArray(xs)) {
  console.log('typed-guard:', xs.length);
}
const probe = Array.isArray();
console.log('zero-arg-Boolean:', Boolean(probe));
console.log('zero-arg-not:', !probe);
