// RFC 20260808-construct-channel B2 — a fn-expr whose body says
// `this`, escaped into the species slot (`a.constructor[Symbol
// .species] = Ctor`), must pass the checker and run with CONSTRUCT
// semantics (§9.4.2.3 ArraySpeciesCreate step 10): fresh `this`
// linked to `Ctor.prototype`. The product's identity is B3
// (species-product retention) — not asserted here.

// variable-routed profile (create-species.js shape, sans arguments)
var thisValue;
var callCount = 0;
var Ctor = function () {
  callCount += 1;
  thisValue = this;
};
var a = [];
a.constructor = {};
a.constructor[Symbol.species] = Ctor;
var result = a.concat();
console.log(callCount);
console.log(Object.getPrototypeOf(thisValue) === Ctor.prototype);
console.log(typeof result);

// inline fn-expr profile, second consult surface (slice)
var protoSeen;
var b = [1, 2, 3];
b.constructor = {};
b.constructor[Symbol.species] = function () {
  protoSeen = Object.getPrototypeOf(this);
};
var sliced = b.slice(1);
console.log(protoSeen !== undefined && protoSeen !== null);
console.log(typeof sliced);
