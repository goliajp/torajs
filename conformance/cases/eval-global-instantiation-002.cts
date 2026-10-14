// §19.2.1.3: the global binding of a function an eval's text declares
// is created when the eval runs, as a property of the global object,
// and the name resolves to that property from then on. Before the
// eval a read, a call or a callback use is a ReferenceError; a
// property the program put there first is what the name reads.
// §B.3.2.3: only a lexical declaration around a block function keeps
// it from its `var` binding. A text evaluated as an expression binds
// its `var`s when it runs, not at program start.
// oracle: node script goal (`vm.runInThisContext`); bun runs a `.cts`
// as CommonJS, where top-level `var` is not global.
try { console.log(f1()); } catch (e) { console.log("call before", e instanceof ReferenceError); }
eval("function f1() { return 1 }");
console.log(f1());
globalThis.f2 = 5;
console.log(f2, typeof f2);
eval("function f2() {}");
console.log(typeof f2);
var xs = [1, 2, 3];
try { console.log(xs.map(g1)); } catch (e) { console.log("map before", e instanceof ReferenceError); }
eval("function g1(x) { return x * 2 }");
console.log(xs.map(g1).join(","), g1(4), typeof g1, g1.name, g1.length);
var h = g1;
console.log(h(5), h === g1, globalThis.g1 === g1);
eval("function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2) }");
console.log(fib(15));
globalThis.g1 = function (x) { return x + 100 };
console.log(g1(1), xs.map(g1).join(","));
function outer() { return typeof f7 }
console.log(outer());
eval("function f7() { return 7 }");
console.log(outer(), f7());
(0, eval)("console.log(typeof f, 'f' in globalThis); { let f = 1 } { function f() {} }");
(0, eval)("console.log('j' in globalThis); try { let j } catch (e) {} finally { function j() {} }");
(0, eval)("console.log('i' in globalThis); switch (1) { case 1: let i; default: { function i() {} } }");
console.log("y" in globalThis);
var r = eval("var y = 2; 3");
console.log(r, y, "y" in globalThis);
