// §15.7 — "All parts of a ClassDeclaration or a ClassExpression are
// strict mode code", and the ClassHeritage is one of those parts.
// `class C extends (function B() { with ({}); return B; }()) {}` is a
// SyntaxError, and so are an octal literal, duplicate parameters and
// an `eval` binding written there.
//
// None of that can appear in a fixture that runs. The obvious runtime
// tell does not work either: §10.2.1.2 gives a plain call in strict
// code `undefined` for `this`, but tr answers with the global object
// inside a heritage (measured) — its own divergence, older than this
// and not what this fixture is for.
//
// What a fixture CAN hold is the other risk, and it is the one a bit
// like this actually creates: leaking past the clause it belongs to.
// Every line below is sloppy-only and sits after a heritage.
//
// bun is not an oracle: it reads a `.cts` as strict, so it refuses
// `with`, `var eval` and `function dup(a, a)` — the three things this
// is here to keep legal. node runs it, so this checks against a
// `.expected`.

var order: string[] = [];

class Sub extends (function () {
  order.push("heritage ran");
  return class {
    tag() {
      return "parent";
    }
  };
})() {
  tag2() {
    return "sub";
  }
}

order.push(new Sub().tag());
order.push(new Sub().tag2());

// The bit must be restored: each of these is legal sloppy code that a
// leaked class-strictness would refuse.
var o = { hit: "with-works" };
with (o) {
  order.push(hit);
}

var eval = "eval-as-a-name";
order.push(eval);

function dup(a, a) {
  return a;
}
order.push("dup " + dup(1, 2));

// A second class, to show the first one's clause did not latch.
class Again extends (function () {
  return class {};
})() {}
order.push("again " + typeof Again);

for (var i = 0; i < order.length; i++) {
  console.log(order[i]);
}
