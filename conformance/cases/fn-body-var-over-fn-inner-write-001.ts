// A `var z; function z() {}` pair of a function body shares one
// binding (§10.2.11 steps 27-28). A write to `z` changes that binding
// only where `z` resolves to it: an inner function's own parameter,
// `let` or `var` named `z`, a block's `let z`, a catch parameter or a
// `for (let z …)` head is another binding (§9.1.2.1
// GetIdentifierReference walks outward from the innermost scope), and
// the pair keeps the function's value.

function innerLet() {
  var z;
  function z() { return 1; }
  function inner() { let z = 1; z = 2; return z; }
  return typeof z + ":" + z() + ":" + inner();
}
console.log("inner-let", innerLet());

function innerParam(q: number) {
  var z;
  function z() { return 2; }
  const k = (z: number) => { z = q + 1; return z; };
  return typeof z + ":" + z() + ":" + k(0);
}
console.log("inner-param", innerParam(4));

function innerVar() {
  var z;
  function z() { return 3; }
  function inner() { var z = 0; z++; return z; }
  return typeof z + ":" + z() + ":" + inner();
}
console.log("inner-var", innerVar());

function blockLet() {
  var z;
  function z() { return 4; }
  { let z = 1; z += 1; }
  for (let z = 0; z < 2; z++) {}
  try { throw 1; } catch (z) { z = 2; }
  return typeof z + ":" + z();
}
console.log("block-let", blockLet());

// a write from an inner function that does NOT bind `z` reaches it
function innerReaches() {
  var z;
  function z() { return 5; }
  function set() { z = 9; }
  var before = typeof z;
  set();
  return before + ":" + z;
}
console.log("inner-reaches", innerReaches());
