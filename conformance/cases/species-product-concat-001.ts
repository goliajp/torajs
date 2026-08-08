// RFC 20260808-construct-channel B3 (first cut) — concat derives
// INTO the @@species construct product (§23.1.3.1 step 2
// ArraySpeciesCreate → §9.4.2.3 step 10 Construct(C, « len »)):
// element writes ride the any-lane index-set kernel against the
// foreign product, then length. The other six family methods keep
// the default product until their own cut.

// species ctor answers an explicit object → that IS the result
var instance = [];
var Ctor = function () {
  return instance;
};
var a = [1, 2];
a.constructor = {};
a.constructor[Symbol.species] = Ctor;
var r1 = a.concat([3, 4], 5);
console.log(r1 === instance);
console.log(r1.length);
console.log(r1[0], r1[4]);

// species ctor with no return → the fresh construct `this` is the
// result (a plain object, not an Array); elements land as decimal
// keys and length as a data property
var thisVal;
var C2 = function () {
  thisVal = this;
};
var b = [7];
b.constructor = {};
b.constructor[Symbol.species] = C2;
var r2 = b.concat(8);
console.log(r2 === thisVal);
console.log(Array.isArray(r2));
console.log(r2.length);
console.log(r2[0], r2[1]);
