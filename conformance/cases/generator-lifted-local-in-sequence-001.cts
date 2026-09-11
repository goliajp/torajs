// A generator's locals move onto the state-machine object, and every
// reference has to be rewritten with them. The walk that does it had
// no arm for `Expr::Sequence`, so a reference reached only through a
// comma expression kept naming a binding that no longer exists —
// "unknown identifier `log`" at check time, a ReferenceError at run
// time. `Expr::Delete` and `Expr::NewDynamic` were missing for the
// same reason and joined it; a walk that answers for one operand
// shape has to answer for every shape that holds an operand. Those
// two have no fixture here — a lifted `delete` receiver reaches the
// delete lane's own Struct boundary, and a lifted `new` callee reaches
// an unrelated hole in the state machine's field typing — but before
// the arms existed neither reference was rewritten at all, so both
// answered about a binding that was gone.
function* seqInit() {
  var log = [];
  var x = (log.push("a"), 2);
  yield x;
  yield log.join("");
}

function* seqYieldOperand() {
  var log = [];
  yield (log.push("b"), 3);
  yield log.join("");
}

function* seqLet() {
  let log = [];
  let x = (log.push("c"), 4);
  yield x;
  yield log.join("");
}

function* seqNested() {
  var log = [];
  var x = ((log.push("d"), log.push("e")), 5);
  yield x;
  yield log.join("");
}

function drive(f, n) {
  var it = f();
  var out = [];
  for (var i = 0; i < n; i++) {
    out.push(it.next().value);
  }
  return out.join("|");
}

console.log("seq-init    ", drive(seqInit, 2));
console.log("seq-operand ", drive(seqYieldOperand, 2));
console.log("seq-let     ", drive(seqLet, 2));
console.log("seq-nested  ", drive(seqNested, 2));
