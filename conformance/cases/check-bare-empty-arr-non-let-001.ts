// P0.10 — bare `[]` in non-let-init expression position defaults
// to `Array<Any>`, mirroring the LetDecl empty-`[]` default
// shipped earlier. Pre-fix tora rejected the empty literal in
// any non-let position with 'empty array literal needs a type
// annotation', blocking patterns like:
//
//   new Array().length          // 0
//   [].length                   // 0
//   foo([])                     // pass an empty array as arg
//   { x: [] }                   // empty array in obj literal
//
// Test262 uses these pervasively (~50+ cases unblocked across
// the broader sample under built-ins/Array/* and length cases).
//
// Implementation:
// * check.rs Expr::Array arm — when elements is empty AND no
//   surrounding annotation propagates, default the type to
//   `Type::Array(Type::Any)`.
// * ssa_lower.rs Expr::Array arm — same default; intern the
//   `Array<Any>` layout via intern_arr_layout and emit an
//   `arr_alloc_any(0)` call. Same boxing substrate as bare-
//   empty in let-init position uses.

console.log(new Array().length)              // 0
console.log([].length)                       // 0
console.log(new Array(0, 1, 0, 1).length)    // 4

// Bare empty in fn-arg position.
function takeArr(xs: any[]): number { return xs.length; }
console.log(takeArr([]))                     // 0

// Bare empty as RHS of obj literal field.
let obj: any = { ys: [] }
console.log("obj-ok")                        // obj-ok

// Bare empty followed by chained .length / iter — exercises the
// downstream Array<Any> read path.
let count: number = 0
for (let i: number = 0; i < [].length; i = i + 1) { count = count + 1; }
console.log(count)                           // 0
