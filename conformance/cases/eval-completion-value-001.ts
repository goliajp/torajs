// A multi-statement direct eval whose final statement is an expression
// completes with that expression's value (§14.5.1); the desugar places
// it as an IIFE. var declarations die with the eval's own environment
// (strict direct eval, §19.2.1.1).
function f(): number { return 7; }
var r1 = eval("f(); 2;");
var r2 = eval("var q = 5; q + 1;");
class P { x = 10; m() { return eval("this.x; this.x + 1;"); } }
var executed = false;
class C { y = eval("executed = true; 5;"); }
var w = 0;
var r3 = eval("eval('w = 3;'); w + 1;");
var f2 = eval("1; () => 42;");
function boom(): number { throw new Error("k"); }
var caught = "";
try {
  eval("boom(); 2;");
} catch (e) {
  caught = (e as Error).message;
}
console.log(r1, r2, typeof q, new P().m(), new C().y, executed, r3, w, f2(), caught);
