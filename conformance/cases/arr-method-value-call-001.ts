// Array-receiver builtin method VALUE reads reify off the Array
// prototype (mv family gate) — the Function.prototype surfaces
// (.call / .apply / .bind) on them re-dispatch the carried mid with
// the thisArg as receiver, reaching the ES "intentionally generic"
// array-like arm on plain-object receivers.
function fn(e: any) {
  return [39, e * 2];
}
const a: any = { length: 3, 0: 1, 2: 21 };
console.log([].flatMap.call(a, fn));
console.log([].flat.call({ length: 2, 0: [1, 2], 1: 3 }));
console.log([].flat.apply({ length: 2, 0: [1], 1: 2 }));
console.log([].flatMap.apply(a, [(e: any) => [e, e]]));

// The read itself is a first-class function value.
const m = [10, 20].map;
console.log(typeof m);
console.log(m.call([7, 8], (x: number) => x + 1));
console.log([1, 2, 3].slice.call([4, 5, 6], 1));
console.log([].indexOf.call({ length: 3, 1: "x" }, "x"));

// bind carries the thisArg through the mint.
const b2 = [9].includes.bind([1, 9]);
console.log(b2(9));

// RequireObjectCoercible — a nullish bound receiver throws at call.
const bound = [].flat.bind(null);
try {
  bound();
} catch (e: any) {
  console.log("caught:", e.constructor.name);
}
