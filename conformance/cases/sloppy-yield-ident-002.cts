// §12.7.2 misc faces — `yield` as accessor property name (legal under
// BOTH goals, §12.7.6 IdentifierName), as a function-expression name
// ([~Yield], §15.2), and as a fn / generator DECLARATION name (the
// BindingIdentifier inherits the enclosing [Yield] parameter, §15.5).
var sink = 0;
var obj = {
  get yield() { return 40 + 2; },
  set yield(v) { sink = v; }
};
console.log(obj.yield);
obj.yield = 7;
console.log(sink);

// Function-expression self-name (dropped binding, but must parse).
var f = (function yield() { return 3; });
console.log(f());

// Generator declaration named `yield` — outside a generator the name
// is plain sloppy-legal; inside its own body `yield` is the keyword.
function* yield(n) {
  yield n + 1;
}
var iter = yield(9);
var r = iter.next();
console.log(r.value);
console.log(r.done);
console.log(iter.next().done);
