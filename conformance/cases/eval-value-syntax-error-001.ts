// A value-position eval of a non-parsing source throws SyntaxError at
// evaluation time (§19.2.1.1 step 12) — carried by an arrow IIFE,
// since JavaScript has no throw expression. Both call forms.
var got = "";
try {
  var r = "x" + eval("(((");
  got = "no-throw:" + r;
} catch (e) {
  got = (e instanceof SyntaxError) ? "syntaxerror" : "other";
}
console.log(got);
var got2 = "";
try {
  got2 = "" + (0, eval)("break;");
} catch (e) {
  got2 = (e instanceof SyntaxError) ? "syntaxerror" : "other";
}
console.log(got2);
console.log(eval("1; 2;"));
