// fn-expr `this` — the IIFE arm (rotation 328): a this-bearing
// function expression standing as its own call's callee promotes,
// and the receiverless call site seeds `undefined` (§10.2.1.2 strict
// call-site `this`, bun module framing). Both spellings covered.
var g = function () {
  return this;
}();
console.log(typeof g);
console.log(g === undefined);
(function () {
  console.log(typeof this);
}());
console.log((function () {
  if (this === undefined) {
    return "strict-this";
  }
  return "other";
})());
// args must not shift around the seeded `__this` slot
console.log((function (a: number, b: number) {
  return this === undefined ? a + b : 0;
})(3, 4));
console.log("after");
