// RC-1 (RFC 20260706-test262-bug-corpus): a callback that never
// returns a value yields `undefined` — ToBoolean(undefined) folds
// every predicate hit to false, map elements become undefined, and
// the reduce accumulator degrades to undefined after the first call.
// Every form below used to SIGTRAP (exit 133): the closure-param
// inference pass seeded a value ret ann onto the valueless body, so
// the lowered fn fell off the end of a value-returning signature.
const nums = [1, 2, 3];
console.log(nums.filter(function () {}).length);
console.log(nums.some(function () {}), nums.every(function () {}));
console.log(nums.findIndex(function () {}), nums.findLastIndex(function () {}));
const m1 = nums.map(function () {});
console.log(m1.length, m1[0]);
console.log([5].map(() => {}).length);
console.log(nums.reduce(function () {}));
console.log(nums.reduce(function () {}, 0));
const anys: any[] = [11, "x"];
console.log(anys.find(function () {}));
console.log(anys.map(function () {}).length);
console.log(anys.filter(function () {}).length);
let hits = 0;
nums.forEach(function () {
  hits = hits + 1;
});
console.log(hits);
