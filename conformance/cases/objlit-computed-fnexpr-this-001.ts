// objlit computed-key fn-expr VALUE binds call-site `this` (§10.2.1.2) —
// the name-keyed field (RFC 20260725) and the computed-key method
// shorthand both already do; this pins the computed-key fn-expr twin.
var o1 = { [Symbol.iterator]: function () { return this; } };
console.log(o1[Symbol.iterator]() === o1);

// literal-string whole key folds at parse time — struct lane
var o2 = { ["folded"]: function () { return this; } };
console.log(o2.folded() === o2);

// runtime computed key — dynobj lane, receiver-first promote
var key = "dyn";
var o3 = { [key]: function () { return this; } };
console.log(o3.dyn() === o3);

// this-free fn-expr under a computed key keeps the plain closure ABI
var o4 = { [key]: function (a: number, b: number) { return a + b; } };
console.log(o4.dyn(3, 4));
