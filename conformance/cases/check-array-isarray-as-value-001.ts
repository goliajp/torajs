// T-38-followup — `Array.isArray` used as a value (not in callee
// position). Pre-fix ssa_lower errored "unknown ident `Array`"
// because tora's `Array` is a typecheck-only namespace marker with
// no SSA operand. AST desugar rewrites every non-callee
// `Array.isArray` Member to a synth stub FnDecl
// `__torajs_array_isarray_stub(x: any): boolean`; call-site uses
// `Array.isArray(value)` stay routed through the existing static-
// check intercept. Unblocks the two 5k cases that grab the function
// as a value or read its `.length`.

// Capture as value, check typeof.
let f: (x: any) => boolean = Array.isArray;
console.log(typeof f);             // "function"

// Direct .length access (no capture).
console.log(Array.isArray.length); // 1

// Call-site path still works through the existing intercept.
console.log(Array.isArray([1, 2, 3]));  // true
console.log(Array.isArray("nope"));      // false
console.log(Array.isArray(Math));        // false (T-38 namespace short-circuit)
