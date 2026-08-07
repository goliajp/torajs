// fn-expr `this` — the variable-routed ALL-DIRECT-CALL profile
// (rotation 328): a binding whose init is a this-bearing function
// expression and whose every use is a direct call promotes; the call
// seeds `undefined` (strict call-site `this`, bun module framing).
var f = function () {
  return this;
};
console.log(typeof f());
console.log(f() === undefined);
var g = function () {
  return typeof this;
};
console.log(g());
// a this-free fn-expr binding keeps the plain ABI untouched
var h = function () {
  return 7;
};
console.log(h());
// declared but never called — promotion still clears the reject
var unused = function () {
  return this;
};
console.log("after");
