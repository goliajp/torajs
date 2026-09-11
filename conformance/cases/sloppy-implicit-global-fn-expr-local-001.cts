// §9.1.1.4.6 — a sloppy assignment to an unresolvable name creates a
// global; one to a name some enclosing scope DECLARES does not. The
// implicit-global synthesis collected declarations off the statement
// spine only, so a function expression's locals (an IIFE's `let x`)
// were invisible to it: `x = 1` written before the `let x` read as a
// global write, a `var x` was invented, and the real `let x` was
// refused as a redeclaration. It is a TDZ write.
(function () {
  try {
    x = 1;
    console.log("BAD: no throw");
  } catch (e) {
    console.log("tdz", e instanceof ReferenceError);
  }
  let x = 5;
  console.log("x", x);
})();

// A name nothing declares still becomes a global — including one
// that some function expression happens to declare as ITS local.
(function () {
  function g() {
    implicitOne = 7;
  }
  g();
})();
console.log("global", implicitOne);
const h = function () {
  let y = 2;
  return y;
};
console.log("before", typeof y);
y = 9;
console.log("after", y, h());
