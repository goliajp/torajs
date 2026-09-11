// §10.2.11 FunctionDeclarationInstantiation step 28 — a `var` that
// re-declares a parameter is the parameter's own binding: a bare
// `var a;` declares nothing new and `var a = e` writes it. The
// hoist pass minted a fresh prelude slot for the name, which the
// checker then refused as a redeclaration.
function f1(a = 0) {
  var a;
  return a;
}
console.log(f1(), f1(5));

function f2(a) {
  var a = a + 1;
  return a;
}
console.log(f2(1));

function f3(a, b) {
  if (b) {
    var a = "in block";
  }
  return a;
}
console.log(f3("param", false), f3("param", true));

function f4(a) {
  var a;
  var a;
  return a;
}
console.log(f4(9));
