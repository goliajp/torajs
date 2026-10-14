// §19.2.1.3: a function an eval's text declares binds the global
// name, and a bare occurrence reaches that binding only where no
// local declaration of the name is in scope. A loop variable
// (§14.7.5.7), a `for (let …;;)` binding, a catch parameter, a class
// expression's name, a parameter and a block's `let` each shadow it
// for their own extent; past it the name is the global function again.
// oracle: node script goal (`vm.runInThisContext`); bun runs a `.cts`
// as CommonJS, where top-level `var` is not global.
eval("function f1() { return 'f1' }");
for (const f1 of [10]) console.log(f1);
console.log(f1());
eval("function f2() { return 'f2' }");
for (let f2 in { a: 1, b: 2 }) console.log(f2);
console.log(f2());
eval("function f3() { return 'f3' }");
for (let f3 = 0; f3 < 2; f3++) console.log(f3);
console.log(f3());
eval("function f4() { return 'f4' }");
try { throw 4 } catch (f4) { console.log(f4) }
console.log(f4());
eval("function f5() { return 'f5' }");
const K = class f5 { static m() { return typeof f5 } };
console.log(K.m(), f5());
eval("function f6() { return 'f6' }");
function p(f6) { return f6 }
console.log(p(6), f6());
eval("function f7() { return 'f7' }");
{ let f7 = 7; console.log(f7) }
console.log(f7());
eval("function f8() { return 'f8' }");
{ for (const f8 of "a,b".split(",")) console.log(f8) }
console.log(f8());
eval("function f9() { return 'f9' }");
for (const [f9] of [[9]]) console.log(f9);
console.log(f9());
eval("function f10() { return 'f10' }");
const fs = [];
for (const f10 of [1, 2]) fs.push(() => f10);
console.log(fs.map((g) => g()).join(","), f10());
eval("function f11() { return 'f11' }");
const arrow = (f11) => f11 + 1;
console.log(arrow(11), f11());
