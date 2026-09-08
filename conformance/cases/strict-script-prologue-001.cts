"use strict";
// ES §11.2.2 — a prologue makes a SCRIPT strict without changing its
// goal, and every rule phrased "in strict code" then applies: the
// arguments object is unmapped (§10.4.4) and an undefined receiver
// stays undefined (§10.2.1.2). Neither is about being a module.
function unmapped(a, b) {
  a = 99;
  return arguments[0] + "," + arguments[1];
}
console.log(unmapped(1, 2));

function receiver() {
  return this === undefined ? "undefined" : typeof this;
}
console.log(receiver());
