// RFC 20260801-arguments-method-face — declare-then-assign function
// expressions join the arguments faces: `var ref; ref = function...`
// folds to `let ref = <closure>` in desugar_uninit_let, and the
// orphaned Assign/Ident arena nodes are tombstoned so the
// binding-safety walk no longer kills the chain (test262
// func-expr-args-trailing-comma family).
var callCount = 0;
var ref;
ref = function () {
  console.log(arguments.length);
  console.log(arguments[0]);
  console.log(arguments[1]);
  callCount = callCount + 1;
};
ref(42, "TC39",);
console.log(callCount);

// length-only body (argc tier) through the same fold
var lenOnly;
lenOnly = function () {
  console.log(arguments.length);
};
lenOnly(7, 8, 9);

// let-declared variant
let refLet;
refLet = function () {
  console.log(arguments[0], arguments.length);
};
refLet("x");
