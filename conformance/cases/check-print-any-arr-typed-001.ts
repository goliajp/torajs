// Nested-print substrate trunk Commit 4 acceptance — non-empty
// typed Arr<Any>. SSA dispatcher (Type::Arr(arr_id), false) elem
// Type::Any → print_any → __torajs_print_anyv Tag::Arr branch
// (Commit 4 wire) → __torajs_arr_print_any walker (Commit 2) →
// each slot through __torajs_print_anyv_inline (Commit 1)
// dispatching on each NaN-box AnyValue tag.
const a: any[] = [1, 2, 3];
console.log(a);
const b: any[] = ["hi", true, null];
console.log(b);
