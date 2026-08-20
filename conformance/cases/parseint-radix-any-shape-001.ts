// §19.2.5.1 step 2 runs ToInt32 on the radix, and ToInt32's ToNumber
// takes any value at all. tr used to answer that question with an
// integer-SHAPE guard instead: a string radix was a compile-time type
// error, and an `as any` cast over a heap value (which is a
// value-layer pass-through, so the SSA type stays Obj) reached the
// coerce and rejected. Both spellings run in every engine.

console.log(parseInt("11", "16"), Number.parseInt("11", "16"));
console.log(parseInt("ff", "16"), Number.parseInt("ff", "16"));

// an `as any` cast over a heap value keeps its static type
var withValueOf = { valueOf() { return 16; } };
console.log(Number.parseInt("11", withValueOf as any));
console.log(parseInt("11", withValueOf as any));

var withToString = { toString() { return "2"; } };
console.log(Number.parseInt("101", withToString as any));

// arrays coerce through ToPrimitive too: [8] -> "8" -> 8
console.log(Number.parseInt("17", [8] as any));

// booleans: ToInt32(true) is 1, which is not a legal radix -> NaN
console.log(Number.parseInt("11", true as any));
console.log(Number.parseInt("11", false as any));

// the auto-detect and explicit-undefined paths keep answering
console.log(parseInt("0x1f"), Number.parseInt("0x1f", undefined));
console.log(Number.parseInt("11", 16), Number.parseInt("11"));

// a radix whose valueOf throws propagates
try {
  Number.parseInt("11", { valueOf() { throw new Error("radix"); } } as any);
} catch (e) {
  console.log("caught", (e as any).message);
}

// the string is still read before the radix coerces
var order: string[] = [];
var s = { toString() { order.push("s"); return "11"; } };
var r = { valueOf() { order.push("r"); return 16; } };
console.log(Number.parseInt(s as any, r as any), order.join(","));
