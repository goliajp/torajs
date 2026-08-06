// A builtin prototype's READABLE surface is its own methods plus
// everything it inherits from `Object.prototype` — the chain every
// builtin prototype's [[Prototype]] ends at. Reading a name the
// family does not own used to answer undefined, while asking the
// same question with `in` walked the chain and answered true.

// The families that inherit `toString` rather than own it (§27.2.5 /
// §24.1.3 / §24.2.3 / §24.3.3 / §24.4.3 brand themselves through
// Symbol.toStringTag instead).
console.log(typeof Promise.prototype.toString, typeof Map.prototype.toString);
console.log(typeof Set.prototype.toString, typeof WeakMap.prototype.toString);
console.log(typeof WeakSet.prototype.toString);
console.log(Map.prototype.toString === Object.prototype.toString);
console.log(Set.prototype.toString === Object.prototype.toString);
console.log(WeakMap.prototype.toString === Object.prototype.toString);
console.log(Promise.prototype.toString === Object.prototype.toString);

// The ones that DO own it keep their own function object.
console.log(typeof Array.prototype.toString, typeof Date.prototype.toString);
console.log(Array.prototype.toString === Object.prototype.toString);
console.log(Date.prototype.toString === Object.prototype.toString);

// Same split for toLocaleString (Number / Object / Array / BigInt /
// Date own one; everyone else inherits).
console.log(typeof Map.prototype.toLocaleString);
console.log(typeof RegExp.prototype.toLocaleString);
console.log(typeof Symbol.prototype.toLocaleString);
console.log(Map.prototype.toLocaleString === Object.prototype.toLocaleString);
console.log(Symbol.prototype.toLocaleString === Object.prototype.toLocaleString);
console.log(Array.prototype.toLocaleString === Object.prototype.toLocaleString);

// The Annex B §B.2.2.2-5 accessor four live on Object.prototype, so
// every family reads the one cell.
console.log(typeof Map.prototype.__defineGetter__);
console.log(typeof Set.prototype.__lookupSetter__);
console.log(
  Map.prototype.__defineGetter__ === Object.prototype.__defineGetter__,
);

// Reading it is one thing; owning it is another. The own face must
// not have moved.
console.log("toString" in Map.prototype, Map.prototype.hasOwnProperty("toString"));
console.log(Object.keys(Map.prototype).length);
console.log(Object.getOwnPropertyDescriptor(Map.prototype, "toString"));

// The inherited read is a chain link, not a copy: removing the one
// implementation removes it for everyone downstream.
delete Object.prototype.toString;
console.log(typeof Object.prototype.toString, typeof Map.prototype.toString);
console.log(typeof Map.prototype.toLocaleString);
