// A function of sloppy code whose own directive prologue says
// "use strict" is strict code (§11.2.2), and its `arguments` object is
// unmapped (§10.2.11 step 22: CreateUnmappedArgumentsObject), however
// many `var`s or function declarations the body holds. Writing a
// parameter does not change `arguments[0]` there, while it does in a
// sloppy function with simple parameters (§10.4.4.7).
// Oracle: node running it as a Script. bun 1.4.3 answers the three
// strict lines as if the bodies were sloppy (`3`, `2:3`, `object1`).

function strictVar(a) { "use strict"; var b = 1; a = 2; return arguments[0] + b; }
console.log("strict-var", strictVar(10));

function strictShared(a) {
  "use strict";
  var z;
  function z() {}
  z = 3;
  a = 2;
  return arguments[0] + ":" + z;
}
console.log("strict-shared", strictShared(10));

function strictThis() { "use strict"; var v = 1; return typeof this + v; }
console.log("strict-this", strictThis());

function sloppyVar(a) { var b = 1; a = 2; return arguments[0] + b; }
console.log("sloppy-var", sloppyVar(10));
