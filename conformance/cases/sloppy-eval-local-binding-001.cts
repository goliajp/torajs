// §13.3.6.1 — `eval(x)` is a direct eval only when the reference resolves
// to %eval%. A strict nested function whose enclosing sloppy function
// declares `var eval` calls that binding as an ordinary function
// (test262 call/tco-non-eval-function).
var callCount = 0;
(function () {
  function f(n: number): any {
    "use strict";
    if (n === 0) {
      callCount += 1;
      return "done";
    }
    return eval(n - 1);
  }
  var eval = f;
  console.log(f(10), typeof eval);
})();
console.log(callCount, typeof eval);
