// S129-5 — Array<Any>.flat(Infinity) ES §23.1.3.13 full-depth
// flatten. Pre-fix tora reject "flat depth must be a number
// literal" at check.rs + ssa-lower (only Expr::Number accepted).
// Infinity is parsed as Expr::Ident("Infinity") not Expr::Number,
// so the literal-only guard rejected the spec-canonical depth.
//
// Fix peels up to 64 layers (both check.rs static peel + ssa-
// lower unroll bound) — matches V8/JSC fixture conventions; no
// realistic nesting reaches that depth. Typed branch breaks
// early once cur_ty stops being Arr<Arr<T>>; the Array<Any>
// branch is no-op shallow clone past the actual depth, so the
// 64x amortization is a constant-factor wash on small inputs.

function getDeep(): any[] { return [1, [2, [3, [4, [5]]]]]; }
const xs: any[] = getDeep();
const flat = xs.flat(Infinity);
console.log(flat.length);  // 5 — fully flattened
console.log(flat[0]);       // 1
console.log(flat[1]);       // 2
console.log(flat[2]);       // 3
console.log(flat[3]);       // 4
console.log(flat[4]);       // 5

// regression — Expr::Number literal still works
console.log(xs.flat(2).length);  // 4 — [1, 2, 3, [4, [5]]]
console.log(xs.flat(3).length);  // 5 — [1, 2, 3, 4, [5]]
console.log(xs.flat().length);   // 3 — default depth=1: [1, 2, [3, [4, [5]]]]

// Array<typed Array<T>>.flat(Infinity) — typed deep-flatten
// peels statically until elem is no longer Array.
const typed: number[][][] = [[[1, 2]], [[3, 4]]];
const tflat = typed.flat(Infinity);
console.log(tflat.length);  // 4
console.log(tflat[3]);       // 4
