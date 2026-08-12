// §13.1.1 on a function's own BindingIdentifier. A function named
// `eval` or `arguments` is refused by STRICT code — including by its
// own body's `"use strict"`, which is why the judgement waits until
// the body has been read. Sloppy script code keeps all of these, and
// that legal side is what a byte-compare fixture can state.
function eval(a: number) {
  return a * 2;
}
console.log(eval(3));

const f = function arguments(a: number): number {
  return a + 1;
};
console.log(f(1));

// The name is judged, the call is not: a nested function named after a
// reserved word stays an ordinary binding here too.
function outer() {
  function statik() {
    return "inner";
  }
  return statik();
}
console.log(outer());
