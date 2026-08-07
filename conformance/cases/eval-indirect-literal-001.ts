console.log((0, eval)("1 + 1"));
console.log((0, eval)("2 * 3") + 1);
console.log((0, eval)(7));
console.log((0, eval)(";"));
console.log((0, eval)("{}"));
(0, eval)("var q = 7;");
console.log(typeof q);
(0, eval)("let L = 9;");
console.log(typeof L);
(0, eval)("var m = 1; m = m + 2;");
console.log(m);
try { (0, eval)("((("); } catch (e) { console.log((e as Error).constructor.name); }
function g() { return (0, eval)("40 + 2"); }
console.log(g());
console.log((0, eval)("'s'") + "!");
console.log((1, eval)("5 - 2"));
