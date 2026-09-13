// 642-08 — two functions each declaring a same-named local closure
// that reads `arguments`, handed back as a value. The return-escape
// admission counted the binding's references by NAME over the whole
// program, so the second owner made the first look shared and neither
// closure got its argv face — `arguments` in both stayed a bare name.
// A local's uses are the uses inside its own function.
function p() { let a = arguments; const f = function () { return a === arguments; }; return f; }
function q() { let a = arguments; const f = function () { return a === arguments; }; return f; }
console.log("dup", p(1)(), q(2)());
// the returned closure reads its own arguments
function r() { const f = function () { return arguments.length; }; return f; }
function s() { const f = function () { return arguments[0]; }; return f; }
console.log("own", r()(1, 2, 3), s()("x", "y"));
// distinct names, as before
function t() { let a = arguments; const g = function () { return a === arguments; }; return g; }
console.log("solo", t(3)());
// a third owner of the same name
function u() { const f = function () { return arguments.length + 10; }; return f; }
console.log("three", p(1)(), r()(1), u()(1, 2));
