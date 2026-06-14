// Nested-print substrate trunk Commit 7 acceptance — Type::Set
// typed-receiver console.log. SSA dispatcher Type::Set →
// __torajs_set_print_outer (dedicated wrapper, same rationale as
// Map: Tag ambiguity at runtime). Set form `Set(N) { v, ... }`,
// empty `Set {}` (no size). Closes Set typed-receiver wedge.
const s: Set<number> = new Set();
s.add(1);
s.add(2);
s.add(3);
console.log(s);
const empty: Set<number> = new Set();
console.log(empty);
