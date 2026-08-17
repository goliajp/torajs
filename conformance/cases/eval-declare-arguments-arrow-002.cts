// declare-arguments arrow lane, SyntaxError half — §19.2.1.3: when a
// parameter of the arrow is itself named `arguments` (legal in sloppy
// code), a default-position direct eval that var-declares `arguments`
// collides with the parameter binding and the CALL throws a
// SyntaxError (t262 arrow-fn-a-{preceding,following}-parameter-is-
// named-arguments-arrow-func-declare-arguments-assign).
var f1 = (p = eval("var arguments = 'param'"), arguments) => {};
try {
  f1();
  console.log("f1: no throw");
} catch (e) {
  console.log("f1 threw:", e instanceof SyntaxError);
}

var f2 = (arguments, p = eval("var arguments = 'param'")) => {};
try {
  f2();
  console.log("f2: no throw");
} catch (e) {
  console.log("f2 threw:", e instanceof SyntaxError);
}
console.log("done");
