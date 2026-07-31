// Rotation 261 — predicate-family HOF thisArg (§23.1.3.8-11/30):
// find / findIndex / findLast / findLastIndex / some / every thread
// the boxed thisArg into a promoted fn-expr predicate's leading
// `__this` slot (arr_ho knife-4 protocol mirrored onto
// ssa_lower_call_arr_predicate).
const t = { th: 5 };
console.log(
  [1, 2, 3].find(function (v) {
    return v + this.th === 7;
  }, t),
); // 2
console.log(
  [1, 2, 3].findIndex(function (v) {
    return v === this.th - 3;
  }, t),
); // 1
console.log(
  [9, 2].some(function (v) {
    return v === this.th;
  }, t),
); // false
console.log(
  [5, 5].every(function (v) {
    return v === this.th;
  }, t),
); // true
console.log(
  [4, 8].findLast(function (v) {
    return v < this.th;
  }, t),
); // 4
console.log(
  [4, 8].findLastIndex(function (v) {
    return v > this.th;
  }, t),
); // 1

// promoted predicate WITHOUT a thisArg — this is undefined
console.log(
  [1, 2].find(function (v) {
    return typeof this === "undefined" && v > 1;
  }),
); // 2

// knife-2 variable-routed predicate face (var spelling)
var p = function (v) {
  return v === this.th;
};
console.log([3, 5].findIndex(p, t)); // 1
