// rc regression — an Any-lane class-candidate member read of a
// refcounted non-Str struct field is a borrow Load; the owned
// release must not steal the slot's stake (second read was a
// dangling Symbol before the fix).
var cases = [
  { sym: Symbol(), str: "Symbol()" },
  { sym: Symbol("ok"), str: "Symbol(ok)" },
];
for (var test of cases) {
  var a = test.sym.toString();
  var b = test.sym.toString();
  var c = Object(test.sym).toString();
  console.log(a === test.str, b === test.str, c === test.str);
}
console.log(cases[1].sym.description);
