// RFC 20260801-arguments-method-face knife 4c — an object-literal
// method whose value escapes into an exclusively-called alias joins
// the argv face (true per-call argc), and that face WINS over the
// static-argv fold when the two would disagree (the fold cannot see
// the alias's calls). A store-only escape keeps the static face.

// escape-only: alias is the sole caller
var o1 = {
  m() {
    console.log(arguments.length, arguments[0], arguments[1]);
  },
};
var r1 = o1.m;
r1(42, "TC39");

// escape + direct call with DIFFERENT argcs — each call answers its
// own true argc (the static fold would have answered 1 to both)
var o2 = {
  m(a: number) {
    console.log(arguments.length, arguments[0], arguments[1]);
  },
};
var r2 = o2.m;
r2(42, "x");
o2.m(7);

// store-only escape (never called): direct sites keep working
// through the static face, over-arity included
var o3 = {
  m() {
    console.log(arguments.length, arguments[0]);
  },
};
var r3 = o3.m;
console.log(typeof r3);
o3.m(5, "z");
