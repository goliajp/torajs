// A SLOPPY direct eval in VALUE position — inside a `for…in` head's
// object literal, a `for…of` head's array literal, a comma operand —
// declares its `var` in the caller's variable environment
// (§19.2.1.3), which at a top-level call site is the global one. tr
// lowers such an eval to an IIFE for its completion value, and the
// IIFE's own frame swallowed the `var`: the program's `x` stayed 1.
// Expected output is node's line for line, run as a Script
// (`vm.runInThisContext`). A name the program declares nowhere gets a
// global of its own, deletable as an eval's var is (§19.2.1.3 step 8).
var probeBefore = function() { return x; };
var x = 1;
var probeDecl, probeExpr, probeBody;
for (let [_ = probeDecl = function() { return x; }] in { '': (eval('var x = 2;'), probeExpr = function() { return x; }) })
  probeBody = function() { return x; };
console.log("for-in", probeBefore(), probeDecl(), probeExpr(), probeBody(), x);
var r = (eval('var fresh = 5; fresh + 1'), fresh);
console.log("fresh", r, typeof fresh, "fresh" in globalThis, Object.getOwnPropertyDescriptor(globalThis, "fresh").configurable);
var q = (eval('var a1 = 3, b1; b1 = a1 * 2; b1'), a1 + b1);
console.log("two-vars", q, a1, b1);
// a `var` statement completes empty (§14.3.2.1): the eval answers
// undefined, so a destructuring default beside it still fires
var pd; var ho = 1;
for (let [_ = pd = function() { return ho; }] of [[eval('var ho = 2;'), 0]]) ;
console.log("empty-completion", pd(), ho, eval('var ho = 3;'));
