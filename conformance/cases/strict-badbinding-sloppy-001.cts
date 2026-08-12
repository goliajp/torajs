// §13.1.1 — `eval` and `arguments` are refused only where STRICT code
// binds them. Sloppy script code keeps every one of these positions as
// an ordinary binding, which is the side a byte-compare fixture can
// state (the strict side is a parse error).
var eval = 10;
console.log(eval);

function f(arguments: number) {
  return arguments + 1;
}
console.log(f(4));

try {
  throw new Error("x");
} catch (eval) {
  console.log((eval as Error).message);
}

function g() {
  var arguments = 7;
  return arguments;
}
console.log(g());
