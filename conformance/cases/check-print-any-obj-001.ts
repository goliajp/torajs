// Nested-print substrate trunk Commit 4 acceptance — Any-binding
// object literal. SSA dispatcher (Type::Any, false) → print_any →
// __torajs_print_anyv Tag::DynObj branch (Commit 4 wire) →
// __torajs_obj_print_any walker (Commit 3) emits the bun
// multi-line `{\n  k: v,\n}` form. Closes the Any-print obj
// fallback wedge.
const o: any = { a: 1, b: 2 };
console.log(o);
