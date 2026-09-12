// Rotation 636 — a function expression (or object-literal method)
// inside a generator body reads the generator's params and locals
// through a lexical alias of the generator object, not through its own
// `this`: an IIFE, an iteration callback, a nested arrow inside the
// function expression, and an object-literal method all see the value.
function* g(x: number) {
  let y = x + 1;
  (function () { console.log("iife", x, y); })();
  [1, 2].forEach(function (k) { console.log("cb", k, x + y); });
  const o = { m() { return x * 10; } };
  console.log("objlit", o.m());
  (function () { const inner = () => x + y; console.log("nested-arrow", inner()); })();
  yield x;
  y = y + 100;
  (function () { console.log("after-yield", x, y); })();
  var f = function () { return y; };
  console.log("let-bound", f());
}
const it = g(10);
console.log(it.next().value);
console.log(it.next().done);
