// `null ?? rhs` / `undefined ?? rhs` pre-fix ran into the typed-
// nullish fall-through that sized the result slot by lhs's SSA
// type (`Type::Ptr` for null literal). A typed rhs (String, Bool,
// Number) was Load'd back at that wrong SSA type — `console.log(
// null ?? 'default')` printed the raw heap pointer integer instead
// of "default". Fix adds a `lhs_always_nullish` short-circuit
// mirroring `lhs_non_nullable`: when check.rs proves lhs is
// `Type::Null` / `Type::Undefined`, ssa_lower lowers rhs at its
// own SSA type and returns directly, so the caller receives the
// rhs-typed value with no slot round-trip.

console.log(null ?? 'default');
console.log(undefined ?? 'default');
console.log(typeof (null ?? 'x'));
console.log((null ?? 'x') === 'x');
console.log((undefined ?? 'y') === 'y');

console.log(null ?? 42);
console.log(undefined ?? 42);
console.log(typeof (null ?? 7));

console.log(null ?? true);
console.log(undefined ?? false);
console.log(typeof (null ?? true));
console.log((null ?? true) === true);
