// §15.5.5 — a YieldExpression's operand is an AssignmentExpression,
// so a comma after it belongs to an enclosing SequenceExpression, not
// to the yield. `yield 1, 2` is `(yield 1), 2` and a bare `yield,
// yield` is two of them. The statement form assumed a yield statement
// was the WHOLE statement and stopped at the comma.
//
// A sequence in statement position discards every value, so the
// elements are just statements in order.
function* pair() {
  yield, yield;
}
function* withOperands() {
  yield 1, 2;
  yield 3;
}
function* effects() {
  var log = [];
  yield 1, log.push("b"), log.push("c");
  yield log.join("");
}

function drive(f, n) {
  var it = f();
  var out = [];
  for (var i = 0; i < n; i++) {
    var r = it.next();
    out.push(r.value === undefined ? "u" : r.value, r.done);
  }
  return out.join(",");
}

console.log("bare-pair ", drive(pair, 3));
console.log("operands  ", drive(withOperands, 3));
console.log("effects   ", drive(effects, 3));

// Still a plain yield statement when nothing follows.
function* plain() {
  yield;
  yield 7;
}
console.log("plain     ", drive(plain, 3));

// And inside an object method generator.
var o = {
  *m() {
    yield, yield 4;
  },
};
console.log("method    ", drive(function () { return o.m(); }, 3));
