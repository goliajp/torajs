// r380 — §10.2.1.2 OrdinaryCallBindThis step 4 under the sloppy
// script goal: a PRIMITIVE receiver is wrapped by ToObject before the
// body sees it, so `typeof this` reads "object" even when the call
// site handed in a number. The prologue used to bind only step 6
// (the undefined/null -> global object half) and let a primitive
// through unwrapped.

function kind() { return typeof this; }

console.log(kind.call(5));
console.log(kind.call("s"));
console.log(kind.call(true));

// the wrapper is a wrapper: the value reads back through it
function unwrap() { return this.valueOf(); }
console.log(unwrap.call(42));
console.log(unwrap.call("hi"));

// and it carries the primitive's own members
function len() { return this.length; }
console.log(len.call("abcd"));

// step 6 half unchanged
function global() { return this === globalThis; }
console.log(global());
console.log(kind.call(null));
console.log(kind.call(undefined));

// an object receiver is handed straight back, identity intact
const target = { x: 0 };
function same() { return this === target; }
console.log(same.call(target));

// receiver writes still land on the caller's object
function bump() { this.x++; return this.x; }
console.log(bump.call(target), target.x);

// a method call keeps its own receiver
const o = { tag: "o", who() { return this.tag; } };
console.log(o.who());
