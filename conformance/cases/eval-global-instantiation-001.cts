// §19.2.1.3 EvalDeclarationInstantiation for global code: a sloppy
// eval's `var`s, its functions and the §B.3.2.3 `var` of its block
// functions are bound when the eval is EVALUATED, at the head of its
// text — not at program start, and not where each declaration sits.
// Before the eval the name is unbound: a read is a ReferenceError and
// `typeof` answers "undefined".
// oracle: node script goal (`vm.runInThisContext`); bun runs a `.cts`
// as CommonJS, where top-level `var` is not global.
console.log(typeof x1);
try { x1; } catch (e) { console.log("read before", e instanceof ReferenceError); }
eval("var x1 = 1");
console.log(x1, typeof x1);
console.log(typeof f1);
try { f1; } catch (e) { console.log("fn before", e instanceof ReferenceError); }
eval("function f1() { return 1 }");
console.log(typeof f1, f1());
eval("initial = x2; var x2;");
console.log(initial, "x2" in globalThis);
(0, eval)("console.log('head', f3, 'f3' in globalThis); { function f3() { return 3 } }");
console.log(typeof f3, f3());
eval("if (true) function f4() { return 4 }; console.log('after if', f4())");
function later() {
  try {
    return typeof y5 + " " + y5;
  } catch (e) {
    return typeof y5 + " " + e.constructor.name;
  }
}
console.log(later());
(0, eval)("var y5 = 'five'");
console.log(later());
x6 = "assigned";
eval("var x6;");
console.log(x6, Object.getOwnPropertyDescriptor(globalThis, "x6").configurable);
// a catch parameter does not stop the block function's binding (§B.3.5)
eval("console.log('catch head', f7); try { throw null; } catch (f7) {{ function f7() { return 7; } }} console.log(typeof f7, f7());");
// the block function's own references stay on its block binding
var initialBV, currentBV;
eval('{ function f8() { initialBV = f8; f8 = 8; currentBV = f8; return "decl"; } }');
f8();
console.log(initialBV(), currentBV, f8());
