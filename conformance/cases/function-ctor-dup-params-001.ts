// CreateDynamicFunction parses its assembled text non-strict, where a
// duplicate parameter name is legal (spec 20.2.1.1 / 15.2.1): the name
// reads the LAST duplicate's argument, the earlier slots still consume
// their positions (.length and the arguments object are positional),
// and the duplicate makes the arguments object unmapped (10.4.4).
const f = Function("a", "a", "return 1;");
console.log(f(9, 8), f.length);

const g = Function("x", "x", "return x;");
console.log(g(1, 2));

const h = new Function("y", "y", "return y;");
console.log(h(3, 4));

const k = Function("a", "a", "return arguments[0];");
console.log(k(7, 8));

const w = Function("a", "a", "arguments[0] = 99; return a;");
console.log(w(1, 2));

// A strict prologue keeps the 15.2.1 creation-time SyntaxError.
try {
  Function("a", "a", "'use strict'; return;");
} catch (e) {
  console.log(e instanceof SyntaxError);
}
