// ES §13.13 — LogicalAND/OR with statically-known nullish lhs.
// `ToBoolean(null) = ToBoolean(undefined) = false`, so:
//   null || rhs → rhs
//   undefined || rhs → rhs
//   null && rhs → null (rhs not evaluated)
//   undefined && rhs → undefined
// Prior to S138 check.rs rejected these as "`&&` / `||` require
// matching operand types" when rhs typed-rhs typed-non-matching.
// Mirrors the typed-lhs `??` fix (S128, 19b04c15).
console.log(null || 'd');
console.log(undefined || 'd');
console.log(null || 42);
console.log(undefined || 42);
// && returns lhs (null/undefined); print "null"/"undefined" path
// is a separate pre-existing gap, so verify via identity.
console.log((null && 'x') === null);
console.log((undefined && 'x') === undefined);
console.log((null && 42) === null);
// Sanity: existing same-type / Any-mixed paths still work.
console.log(true || false);
console.log(5 || 10);
console.log('a' || 'b');
const x: any = 'hello';
console.log(x || 'default');
