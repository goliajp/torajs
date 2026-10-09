// A script's top-level `var` and function declarations ARE properties
// of the global object (§16.1.7), so `globalThis.X` reads the
// program's own value there; a top-level `let`, or a parameter, is not
// one, and the read stays the builtin.
// oracle: node script goal.
var escape = function (s) { return "u:" + s; };
console.log(escape("a b"), globalThis.escape("a b"));
function parseInt(s) { return 42; }
console.log(parseInt("3"), globalThis.parseInt("3"));
let isNaN = 5;
console.log(isNaN, globalThis.isNaN(NaN));
function f(Math) { return globalThis.Math.floor(2.5) + Math; }
console.log(f(1));
function g(process) { return typeof globalThis.process.argv + process; }
console.log(g("!"));
