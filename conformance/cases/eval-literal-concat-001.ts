// eval with a constant string-concatenation argument resolves at
// compile time exactly like a single literal (§13.15.3 String+String).
var a = 0;
eval("a = 1" + "0;");
var b = eval("2" + " + 3");
var c = (0, eval)("'x'" + "+'y'");
var s = 0;
eval(
  'switch (1) {' +
  '  case 1:' +
  '    s = 7;' +
  '}'
);
console.log(a, b, c, s);
