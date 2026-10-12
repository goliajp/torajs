// §19.2.1.1 / §13.3.6.1 — a sloppy direct eval of "var eval = g" lands
// the binding in the calling function's variable environment, so the
// strict nested function's `eval(x)` afterwards is an ordinary call of g
// (test262 call/tco-non-eval-function-dynamic).
var callCount = 0;
(function () {
  function g(n: number): any {
    "use strict";
    if (n === 0) {
      callCount += 10;
      return "done";
    }
    return eval(n - 1);
  }
  eval("var eval = g;");
  console.log(g(7), typeof eval);
})();
console.log(callCount, typeof eval);
