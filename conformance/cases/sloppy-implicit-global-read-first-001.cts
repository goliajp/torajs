// §6.2.5.6 PutValue creates a global binding for an assignment to an
// unresolvable name in sloppy code — but §13.15.2 evaluates the RHS and
// calls GetValue on it FIRST, so an assignment whose own value reads
// the target throws before any binding can exist.
//
// Every compound form is that shape once desugared (`x += 1` is
// `x = x + 1`), and so is a plain assignment that happens to mention
// its own target.
function name(fn) {
  try {
    fn();
    return "no throw";
  } catch (e) {
    return e.name;
  }
}

createdByPlainAssign = 1;
console.log(typeof createdByPlainAssign);

console.log(name(function () { compoundTarget += 1; }));
console.log(name(function () { shiftTarget >>>= 1; }));
console.log(name(function () { ++prefixTarget; }));
console.log(name(function () { postfixTarget++; }));
console.log(name(function () { selfMentioning = 1 + selfMentioning; }));

// A function body is not evaluated by the assignment, so the read
// inside it runs later — if ever — and the binding is created.
heldInBody = function () {
  return heldInBody;
};
console.log(typeof heldInBody);
