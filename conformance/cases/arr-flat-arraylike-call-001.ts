// Array.prototype.flat / flatMap over generic array-like receivers
// via the reified-cell .call re-dispatch (ES §23.1.3.11 / §23.1.3.13
// "intentionally generic"): HasProperty-gated walk (absent keys skip
// entirely), depth decode after the length read, array-valued
// elements spread through the shared flat-depth kernel.
function fn(e: any) {
  return [39, e * 2];
}
const a: any = { length: 3, 0: 1, 2: 21 };
console.log(Array.prototype.flatMap.call(a, fn));

const h1: any = { length: 2, 0: [[1], 2], 1: 3 };
console.log(Array.prototype.flat.call(h1));
console.log(Array.prototype.flat.call(h1, 2));
console.log(Array.prototype.flat.call(h1, 0));
console.log(Array.prototype.flat.call(h1, Infinity));
console.log(Array.prototype.flat.call({ length: 3, 0: 1, 2: [21] }));

// ToLength(undefined) = 0 — empty walk, fresh empty product.
console.log(Array.prototype.flatMap.call({ length: undefined, 0: 9 }, (e: any) => [e]));

// RequireObjectCoercible — a nullish receiver is the TypeError.
try {
  Array.prototype.flat.call(null);
} catch (e: any) {
  console.log("caught:", e.constructor.name);
}
