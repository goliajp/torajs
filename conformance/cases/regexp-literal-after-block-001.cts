// §12.9 — after a `}` that closes a BLOCK the next `/` starts a
// regex literal (a new statement); after a `}` that closes an object
// literal it is division. The lexer tells the two apart by which `{`
// the `}` matched (test262 statementList/block-regexp-literal-*).
{}
/foo/g.test("foo") && console.log("block then regex");
{} /bar/.test("bar") && console.log("same line");
if (true) {} /baz/.test("baz") && console.log("if block");
function f() { return 1; } /q/.test("q") && console.log("function body");
class A {} /c/.test("c") && console.log("class body");
var o = { a: 1 } / 2;
console.log(o);
var p = ({ b: 2 }) / 2;
console.log(p);
var q = f() / 2;
console.log(q);
label: { } /l/.test("l") && console.log("labeled block");
switch (1) { case 1: { } /s/.test("s") && console.log("case block"); }
console.log(typeof ({ k: { m: 1 } }).k.m, [1] / 1);
// A function EXPRESSION's body brace ends an expression, not a
// statement: the `/` after it is division (division/S11.5.2_A3_T1.5).
console.log(isNaN(function () { return 1; } / {}), isNaN({} / function () { return 1; }));
var fe = function () { return 2; } / 2;
console.log(isNaN(fe));
function decl() { return 3; } /z/.test("z") && console.log("declaration body then regex");
