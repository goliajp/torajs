// Nested-print substrate trunk Commit 4 acceptance (W-O-1 empty
// typed Arr<Any>). SSA dispatcher (Type::Arr(arr_id), false) elem
// non-prim arm routes to `print_any` (was `print_i64` raw ptr).
// Closes the W-O-1 wedge: `const a: any[] = []; console.log(a)`
// → "[]" not raw heap pointer decimal.
const a: any[] = [];
console.log(a);
