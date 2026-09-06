// §6.2.5.6 PutValue — in sloppy code an assignment to an unresolvable
// name creates a global binding, and §13.5.1.2 answers `true` for
// `delete` of it (the property is configurable, unlike a declaration's
// binding).
//
// The bare-name triage used to keep such a name OUT of the
// implicit-globals synthesis, so that a later read could still reach
// the ReferenceError a real removal leaves behind. The cost was the
// assignment: with no binding, `x = 1` was an unresolvable write and
// the program died at compile time on an unknown identifier — the
// delete's own answer was never reached either. Three wrong answers
// bought one right one.
//
// What remains is the residue the triage already records for its
// configurable-builtin lane: the answer is right, the property is not
// actually removed, so a read after the delete sees the value rather
// than throwing. That half is deliberately not exercised here.

x = 1;
console.log(delete x);

// The write may sit anywhere; the binding is global either way.
function writes(): void {
  y = 5;
}
writes();
console.log(y, delete y);

// A declared binding is non-configurable — false, and untouched.
var declared = 1;
console.log(delete declared, typeof declared);
let lexical = 2;
console.log(delete lexical, typeof lexical);

// A name that was never written at all is unresolvable: true.
console.log(delete neverWritten);

// Reading an implicit global before and after its write, with no
// delete in sight, is the shape the synthesis exists for.
console.log(typeof beforeWrite);
beforeWrite = 3;
console.log(typeof beforeWrite, beforeWrite);
