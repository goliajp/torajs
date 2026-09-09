// The sibling of `sloppy-implicit-global-read-first-001`: the read that
// beats the write to the binding need not be inside the assignment's
// own value. §13.15.1 and friends evaluate operands left to right, so
// in `x + (x = 1)` the left operand is GetValue'd while `x` is still
// unresolvable — a ReferenceError, and nothing is ever created.
//
// A check that only looked at the assignment's value cannot see this
// one; the question is evaluation order across the whole expression.
function name(fn) {
  try {
    fn();
    return "no throw";
  } catch (e) {
    return e.name;
  }
}

// Read on the left, write on the right — 18 test262 operator cases.
console.log(name(function () { addLeft + (addLeft = 1); }));
console.log(name(function () { mulLeft * (mulLeft = 1); }));
console.log(name(function () { shiftLeft << (shiftLeft = 1); }));
console.log(name(function () { eqLeft === (eqLeft = 1); }));
console.log(name(function () { inLeft in (inLeft = 1); }));
console.log(name(function () { seqLeft, (seqLeft = 1); }));

// An earlier argument is evaluated before a later one.
function twoArgs(a, b) {
  return 0;
}
console.log(name(function () { twoArgs(argLeft, argLeft = 1); }));

// Array and object literals are evaluated element by element.
console.log(name(function () { [arrLeft, (arrLeft = 1)]; }));
console.log(name(function () { ({ a: objLeft, b: (objLeft = 1) }); }));

// The other order creates the binding: the write runs first, and the
// read that follows finds it.
writeFirst = 1;
console.log(writeFirst + (writeFirst = 2));

// `typeof` and `delete` do not read an unresolvable name, so neither
// one is a read that got there first — these still create.
if (typeof guardedByTypeof === "undefined") {
  guardedByTypeof = 3;
}
console.log(guardedByTypeof);
delete deletedFirst;
deletedFirst = 4;
console.log(deletedFirst);
