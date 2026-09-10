// §13.5.1.2 step 5 — a refused delete throws only in STRICT code, and
// strictness has a second source below the program: a `"use strict"`
// directive makes one function strict inside a sloppy Script. The
// delete lane first keyed its kernel flavor on the program (a
// recorded "narrower divergence": `f` answered "no-throw" here); it
// follows the function's goal since rotation 625. `.expected` because
// bun's CommonJS transpile drops a function-level directive in a
// .cts and prints "no-throw" for `f` / `h`; node on the same source
// as .js is the oracle behind the file.
function f() {
  "use strict";
  try {
    delete Object.prototype;
    return "no-throw";
  } catch (e) {
    return "throw " + e.constructor.name;
  }
}
console.log(f());
function g() {
  return delete Object.prototype;
}
console.log(g());
const arr = [1, 2, 3];
function h() {
  "use strict";
  try {
    delete arr.length;
    return "no-throw";
  } catch (e) {
    return "throw " + e.constructor.name;
  }
}
console.log(h());
