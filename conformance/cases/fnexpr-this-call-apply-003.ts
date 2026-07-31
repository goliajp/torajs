// Rotation 261 — the INLINE fn-expr `.call`/`.apply` face
// (`(function () { …this… }).apply(obj)`, the parenthesized IIFE-like
// test262 spelling) plus the §20.2.3.1 absent-argArray form.
const t = { tag: 7 };
console.log(
  (function () {
    return this;
  }).call(t).tag,
); // 7
console.log(
  (function (x) {
    return this.tag + x;
  }).apply(t, [3]),
); // 10
// absent argArray — apply with only a thisArg
console.log(
  typeof (function () {
    return this;
  }).apply(t),
); // object
// absent argArray on a bound face (knife-1/2 replay path)
const f = function () {
  return this.tag * 2;
};
console.log(f.apply(t)); // 14
