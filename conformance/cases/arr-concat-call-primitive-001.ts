// Array.prototype.concat borrowed onto primitive receivers —
// §23.1.3.1 step 1 ToObject(this) seeds the wrapper object
// (test262 concat/call-with-boolean + 15.4.4.4-5-c-i-1 shape).
let r1 = Array.prototype.concat.call(true);
console.log(r1.length, r1[0] instanceof Boolean, String(r1[0]));
let r2 = Array.prototype.concat.call(false);
console.log(r2[0] instanceof Boolean, String(r2[0]));
let r3 = Array.prototype.concat.call(101);
console.log(r3[0] instanceof Number, String(r3[0]));
let r4 = Array.prototype.concat.call("ab");
console.log(r4.length, typeof r4[0], String(r4[0]));
// items: an Array argument spreads, everything else appends whole
let r5 = Array.prototype.concat.call(5, [1, 2], "x");
console.log(r5.length, String(r5[0]), r5[1], r5[2], r5[3]);
// borrow + .call, and .apply with a list
let f = Array.prototype.concat;
let r6 = f.call(7);
console.log(r6.length, r6[0] instanceof Number);
let r7 = Array.prototype.concat.apply("cd", [[9]]);
console.log(r7.length, typeof r7[0], r7[1]);
// the wrapper is a live element of a normal dense product
let r8 = Array.prototype.concat.call(3, 4);
r8[0] = "swapped";
console.log(r8[0], r8[1]);
// own-family lanes unchanged
console.log("ab".concat("c"), [1].concat([2]).length);
console.log(String.prototype.concat.call(5, "x"));
