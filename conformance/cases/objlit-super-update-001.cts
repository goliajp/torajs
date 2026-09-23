// §10.2.4 gives an object-literal method a [[HomeObject]], so
// `super.x++` inside one reads off GetPrototypeOf(home) and writes
// through §9.1.9 OrdinarySet: the base decides whether a setter runs,
// the RECEIVER takes the store otherwise. The four update spellings
// go through the same kernels the class pass uses.

var proto = { x: 1, 2: 5 };

var o1 = { test() { return super.x++; } };
Object.setPrototypeOf(o1, proto);
console.log("post-inc", o1.test(), o1.x, proto.x);

var o2 = { test() { return ++super.x; } };
Object.setPrototypeOf(o2, proto);
console.log("pre-inc", o2.test(), o2.x);

var o3 = { test() { return super[2]--; } };
Object.setPrototypeOf(o3, proto);
console.log("post-dec", o3.test(), o3[2], proto[2]);

var o4 = { test() { return --super[2]; } };
Object.setPrototypeOf(o4, proto);
console.log("pre-dec", o4.test(), o4[2]);

// A key the chain does not hold reads undefined; ToNumeric makes it
// NaN, and the store still lands on the receiver.
var o5 = { test() { super[9]++; } };
o5.test();
console.log("missing", o5[9]);

// A setter on the base runs and NO own property appears on the
// receiver — the half a plain read-modify-write would get wrong.
var seen = [];
var accProto = { get v() { return 10; }, set v(n) { seen.push(n); } };
var o6 = { test() { super.v++; } };
Object.setPrototypeOf(o6, accProto);
o6.test();
console.log("setter", seen.join(","), Object.prototype.hasOwnProperty.call(o6, "v"), o6.v);

// The computed key is evaluated exactly once.
var n = 0;
function k() { n += 1; return "x"; }
var o7 = { test() { return super[k()]++; } };
Object.setPrototypeOf(o7, proto);
console.log("key-once", o7.test(), n);
