// W-G — Object.getOwnPropertyDescriptor(primitive, missing-key) returns undefined
// spec §20.1.2.8 step 1 ToObject(O): undef/null throw (covered by W-D);
// every other primitive boxes to a wrapper. The wrapper has no own property
// matching `"x"`, so the descriptor lookup falls through and returns undefined.
// Acceptance broadening of the reflect.rs:186-199 bun-parity comment.

console.log(Object.getOwnPropertyDescriptor(42, "x"));
console.log(Object.getOwnPropertyDescriptor(true, "x"));
console.log(Object.getOwnPropertyDescriptor("hi", "x"));
console.log(Object.getOwnPropertyDescriptor(42n, "x"));
console.log(Object.getOwnPropertyDescriptor(Symbol("s"), "x"));
