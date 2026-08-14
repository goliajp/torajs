// §13.3.6.2 EvaluateCall — when the callee is a property reference,
// thisValue is the reference's BASE. `arr[0]()` is a method call on
// `arr`, exactly like `o.m()`.
//
// Every leg of the any-lane index call already answered the base — a
// dynobj property, an array's named expando, a string-keyed read —
// except the one the fallback exists for: an ARRAY ELEMENT under a
// canonical-index key, which read the value and bare-called it. So
// `this` read `undefined` there and the object everywhere else.

const arr: any = [];
arr[0] = function () { return (this as any) === undefined };
console.log(1, arr[0]());

// Reading the base back through `this` — the answer the receiver-less
// call could not give.
const box: any = [];
box.tag = "outer";
box[0] = function () { return (this as any).tag };
console.log(2, box[0]());

// Arguments do not shift: the receiver rides its own slot.
const withArgs: any = [];
withArgs[0] = function (a: number, b: number) { return [(this as any).length, a, b] };
console.log(3, withArgs[0](7, 8));

// A dynamic key on the same array — same base, same answer.
const k: any = 0;
console.log(4, box[k]());

// The legs that were already right, pinned beside it so the four stay
// one answer rather than four.
const o: any = {};
o[0] = function () { return (this as any) === undefined };
o.m = function () { return (this as any) === undefined };
const named: any = [];
named.m = function () { return (this as any) === undefined };
const key: any = "m";
console.log(5, o[0](), o.m(), named.m(), o[key]());

// A `this`-free element keeps the plain closure ABI — nothing about
// its call moves.
const plain: any = [];
plain[0] = function (x: number) { return x * 2 };
console.log(6, plain[0](21));

// A builtin read off the array still dispatches for itself.
console.log(7, [3, 1, 2].sort().join("-"));
