// The `with` desugar replaces a whole node — a bare-name call, an
// assignment, `typeof` / `delete` / `++` — with a two-armed guard, and
// each arm mints its own copy of the name. The node the rewrite
// consumed used to be left in the arena with no parent, and a
// parentless `Ident` still reads as a use of that binding to every
// whole-arena analysis.
//
// What that cost: a `this`-using function EXPRESSION promotes its
// receiver only when every use of its binding is a shape the promoted
// ABI survives, and the orphan looked like a use in no recognised
// position. So calling such a function from inside a `with` body
// refused to compile ("closure `__closure_N` references unknown
// identifier `__this`") while the same call one line outside the body
// worked.

var myObj: any = { p2: "obj-p2" };

var reportsThis = function () {
  return typeof this;
};
var counter = 0;
var bump = function () {
  counter = counter + 1;
  return typeof this;
};

with (myObj) {
  // the call shape — this is the one that was refused
  console.log(reportsThis());
  console.log(bump(), counter);
  // the object supplies the name, so this arm calls through it
  console.log(p2);
}

// the same binding still works outside the body
console.log(reportsThis());

// assignment, typeof and ++ over a name the object DOES supply, and
// over one it does not
var n = 5;
with (myObj) {
  p2 = "written";
  n = n + 1;
  console.log(typeof p2, typeof n);
  n++;
}
console.log(myObj.p2, n);

// a name the object does NOT supply falls through to the outer binding
var absent: any = {};
with (absent) {
  n = 99;
  console.log(typeof n);
}
console.log(n, absent.n);
