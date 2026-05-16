// T-38 partial — `Array.isArray(<namespace>)` short-circuit. Pre-fix
// passing a namespace ident (`Math` / `JSON` / `Array` / etc.) as the
// argument hit `ssa-lower: unknown ident <name>` because namespaces
// have no runtime Operand representation in tora's subset (they're
// typecheck-only markers). The full T-38 (treating these namespaces
// as first-class Function values for `var f = Array.isArray` etc.)
// is a deeper L3b item — this short-circuit unblocks the namespace-
// argument shape only.

console.log(Array.isArray(Math));      // false
console.log(Array.isArray(JSON));      // false
console.log(Array.isArray(console));   // false

// Original behavior on actual arrays / non-arrays must not regress.
console.log(Array.isArray([1, 2, 3])); // true
console.log(Array.isArray("not arr"));  // false
console.log(Array.isArray(42));         // false
console.log(Array.isArray(true));       // false
