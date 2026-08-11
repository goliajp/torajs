// RFC 20260810-indirect-argc-abi H1 — head-less top-level fns take
// the hidden sig argc at position 0 (mechanical ABI; readers still
// on the injected param until H2). Direct calls and generic mono
// clones keep answering the true call-site argc. Value escapes and
// beyond/under-arity mono calls stay the checker's loud faces
// (pre-existing, pinned by NOT appearing here).

// exact and under-arity direct calls
function f(a, b) {
  return arguments.length;
}
console.log(f(1));
console.log(f(1, 2));

// zero-param direct
function g() {
  return arguments.length;
}
console.log(g());

// generic mono clone keeps the hidden-slot wiring
function m<T>(v: T, w: T) {
  return arguments.length;
}
console.log(m(1, 2));
console.log(m("a", "b"));
