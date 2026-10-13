// The `var` of a `for (var z of …)` / `for (var z in …)` head
// (§14.7.5), destructuring or not, and of a `for (var z = …; …)` head
// (§14.7.4) is a VarScopedDeclaration of the function body like any
// other, in a generator body too. With a body-level `function z` it
// is one binding (§10.2.11 steps 27-28): the function's value on
// entry, each iteration's value after.

function forOf() {
  var before = typeof z;
  for (var z of [1]) {}
  function z() {}
  return before + ":" + z;
}
console.log("for-of", forOf());

function forIn() {
  var before = typeof z;
  for (var z in { k: 1 }) {}
  function z() {}
  return before + ":" + z;
}
console.log("for-in", forIn());

function forOfEmpty() {
  for (var z of []) {}
  function z() { return 2; }
  return typeof z + ":" + z();
}
console.log("for-of-empty", forOfEmpty());

function dstrHead() {
  var before = typeof z;
  for (var { z } of [{ z: 4 }]) {}
  function z() {}
  return before + ":" + z;
}
console.log("dstr-head", dstrHead());

function cFor() {
  for (var z = 0; z < 1; z++) {}
  function z() {}
  return z;
}
console.log("c-for", cFor());

function* genForOf() {
  yield typeof z;
  for (var z of [1]) {}
  function z() {}
  yield z;
}
console.log("gen-for-of", [...genForOf()].join(","));
