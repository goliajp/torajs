// Argument-bearing Function constructor with constant-string arguments
// resolves at compile time (§20.2.1.1 text assembly); the created
// function's environment is the global environment — it does not see
// the call site's locals.
var add = new Function("a", "b", "return a + b;");
var f = Function("return 42;");
var g = Function("x", "return x * 2;");
function wrap(): string {
  var local = 99;
  var probe = Function("return typeof local;");
  return probe();
}
var h = new Function("n", "if (n < 2) { return 1; } return n * 2;");
console.log(add(2, 3), f(), g(21), wrap(), h(1), h(5), typeof h);
