// callee-side ThisMode ~global~ binding (§10.2.1.2 step 6): a
// detached call of a sloppy function-expression binds `this` to the
// global object — HOF callback with no thisArg, bare direct call of
// a local binding, and the IIFE face.
[10, 20].forEach(function (x) {
  console.log(this === globalThis, x);
});
var f = function () {
  (this as any).__slop_p = 42;
};
f();
console.log((globalThis as any).__slop_p);
console.log((function () { return this === globalThis; })());
