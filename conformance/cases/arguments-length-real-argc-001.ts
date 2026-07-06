// chunk 613 — arguments.length reads the REAL argc in the IIFE closure
// form (static call-site count threaded through a synthetic param,
// T-31 shape) and `arguments` is never mis-collected as a closure
// capture; untyped named-fn return chains infer through fn_sigs
// publish + any fallback instead of collapsing to Void.
function top(a, b, c) {
  return arguments.length;
}
console.log(top(1, 2), top(1, 2, 3));
function outer() {
  function inner(a, b, c) {
    return arguments.length;
  }
  return inner(1, 2);
}
console.log(outer());
console.log(
  (function (a, b, c) {
    return arguments.length;
  })(1, 2)
);
console.log(
  (function (a, b) {
    return arguments.length + arguments[0];
  })(10)
);
function chain(a, b, c) {
  return a;
}
function caller() {
  return chain(7, 8, 9);
}
console.log(caller());
