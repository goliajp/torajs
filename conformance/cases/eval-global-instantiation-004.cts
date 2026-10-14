// §19.2.1.3: a function a global eval's text declares is unbound until
// the eval runs, so before it runs a bare read of the name in a
// function is the global lookup — `typeof` answers "undefined" — unless
// a declaration of the name is in scope there. A lexical binding is in
// scope only in its own block: a loop variable (§14.7.5.7), a block's
// `class` / `let`, a `for (let …;;)` head, a catch parameter, a switch
// clause's `let`, and a block function in strict code (§14.2.6), or one
// §B.3.3.1 gives no `var` because an enclosing block declares the name
// lexically. A sloppy block function, and a `var` loop head, bind the
// function's own scope.
// oracle: node script goal (`vm.runInThisContext`); bun runs a `.cts`
// as CommonJS, where top-level `var` is not global.
function g() { for (const f1 of [1]) {} return typeof f1; }
function h() { { class f1 {} } return typeof f1; }
function k() { for (let f1 = 0; f1 < 1; f1++) {} return typeof f1; }
function m() { for (const f1 in { a: 1 }) {} return typeof f1; }
function n() { try { throw 1; } catch (f1) {} return typeof f1; }
function q() { { let f1 = 1; const z = f1; } return typeof f1; }
function a() { for (const [z, f1] of [[1, 2]]) {} return typeof f1; }
function b() { "use strict"; { function f1() {} } return typeof f1; }
function c() { { function f1() {} } return typeof f1; }
function d() { for (var f1 of [7]) {} return typeof f1; }
function e() { switch (1) { case 1: let f1 = 2; } return typeof f1; }
function f() { { let f1; { function f1() {} } } return typeof f1; }
function i() { if (true) { class f1 {} } try { return f1(); } catch (x) { return x.constructor.name; } }
const t = () => { for (let f1 = 0; f1 < 1; f1++) {} return typeof f1; };
const all = () => [g(), h(), k(), m(), n(), q(), a(), b(), c(), d(), e(), f(), i(), t()].join(" ");
console.log(all());
eval("function f1() { return 1; }");
console.log(all());
