// Unmapped arguments (ES §10.4.4.6/7) — a fn with a default / rest /
// destructured param gets an UNMAPPED arguments object: writes to
// arguments[i] never alias the param (and vice versa). The mapped
// face's literal-index substitution wrote through — test262
// unmapped/via-params-dflt observed the param mutate.
var value = 0;
function dflt(a: any, b: any = 0) {
  arguments[0] = 2;
  value = a;
}
dflt(1);
console.log(value);

// element write must show in later reads and spreads of arguments
function dflt2(a: any, b: any = 0) {
  arguments[0] = 7;
  console.log(arguments[0], arguments.length, a);
}
dflt2(5);
