// RFC 20260808-construct-channel B3 cut 2 — slice / splice / map /
// filter derive their default kernel product, then TRANSPLANT its
// elements into the @@species construct product (§9.4.2.3 step 10).
// Receiver side effects (splice's in-place mutation) are the
// kernel's own; only the product identity swaps. Length lands only
// where the spec has a Set(A,"length") step: slice §23.1.3.4 /
// splice §23.1.3.31 — map / filter / flat only CreateDataProperty
// their elements, so a non-Array product keeps no length entry.

// slice into an explicit returned instance
var inst1 = [];
var C1 = function () {
  return inst1;
};
var a: any = [1, 2, 3, 4];
a.constructor = {};
a.constructor[Symbol.species] = C1;
var r1 = a.slice(1, 3);
console.log(r1 === inst1, r1.length, r1[0], r1[1]);

// map into the fresh construct this (plain object, NO length)
var tv;
var C2 = function () {
  tv = this;
};
var b: any = [5, 6];
b.constructor = {};
b.constructor[Symbol.species] = C2;
var r2 = b.map(function (x: any) {
  return x * 10;
});
console.log(r2 === tv, Array.isArray(r2), r2.length, r2[0], r2[1]);

// splice: receiver mutates in place, removed elements + length land
// in the product
var inst3 = [];
var C3 = function () {
  return inst3;
};
var c: any = [7, 8, 9];
c.constructor = {};
c.constructor[Symbol.species] = C3;
var r3 = c.splice(1, 2);
console.log(r3 === inst3, r3.length, r3[0], r3[1]);
console.log(c.length, c[0]);

// filter — element writes only, no length step
var inst4 = [];
var C4 = function () {
  return inst4;
};
var d: any = [1, 2, 3];
d.constructor = {};
d.constructor[Symbol.species] = C4;
var r4 = d.filter(function (x: any) {
  return x !== 2;
});
console.log(r4 === inst4, r4.length, r4[0], r4[1]);
