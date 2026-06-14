// Nested-print substrate trunk Commit 7 acceptance — Type::Map
// typed-receiver console.log. SSA dispatcher Type::Map →
// __torajs_map_print_outer (bypasses AnyValue tag-walker because
// runtime Tag::Map=15 covers both Map and Set). Empty form omits
// size in parens (`Map {}`) vs non-empty `Map(N) { ... }`. Closes
// Map typed-receiver wedge.
const m: Map<string, number> = new Map();
m.set("a", 1);
m.set("b", 2);
console.log(m);
const empty: Map<string, number> = new Map();
console.log(empty);
