// A type parameter bound from two argument positions takes the JOIN of
// what it saw, and `undefined` stands beside every concrete type as
// its absence — TS binds `T` for `same(xs[5], undefined)` as
// `number | undefined`. The join table already had rows for Null
// (Nullable, Array, Nullable-Nullable) and not one for Undefined, so
// the pair was a hard conflict: "type parameter `T` was inferred as
// Number earlier but here is Undefined". Any is tr's one repr that
// carries both.
function same<T>(a: T, b: T): boolean {
  return (a as any) === (b as any);
}
const xs = [0, 1, 2];
console.log(same(xs[0], 0));
console.log(same(xs[5], undefined));
console.log(same(undefined, xs[5]));
console.log(same("a", undefined));
console.log(same(undefined, undefined));
console.log(same(undefined, null));

function pick<T>(a: T, b: T): any {
  return (a as any) === undefined ? b : a;
}
console.log(pick(7, undefined));
console.log(pick(undefined, 9));
console.log(String(pick([1, 2], undefined)));

// distinct concrete types still conflict — this stays a rejection,
// so the row above is about the nullish and nothing wider.
console.log(same(1, 2));
console.log(same("x", "y"));
