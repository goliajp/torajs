// A `this`-using function expression in a slot that carries the
// FIRST initializer's function type, and that the program later
// writes.
//
// An `any` slot was already safe: its calls ride a receiver-aware
// lane, so a later value that is not promoted answers for itself. A
// typed slot's calls take the bare indirect call shaped by that
// signature — and two stored functions can share a user face while
// differing in whether they carry the receiver slot, which the face
// census cannot see, because it runs before the receiver promotion
// is decided. So such a binding is handed to the boxed dual entry,
// the lane a face-mismatched slot already rides, and its receiver
// question becomes a runtime one.

// Same user face on both sides — only the receiver slot differs.
var a = function () { return typeof this; };
a = function () { return "plain"; };
console.log(a());

// Different faces as well, so both the census and this rule apply.
var b = function () { return typeof this; };
b = function (n) { return n; };
console.log(b(7));

// Never rebound: still the receiverless answer (§10.2.1.2).
var c = function () { return typeof this; };
console.log(c());

// The promoted cell is the one that runs when the write comes after.
var d = function () { return typeof this; };
console.log(d());
d = function () { return "after"; };
console.log(d());
