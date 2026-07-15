// RFC 20260716-primitive-wrapper-substrate 刀 2b — `new String(x)`
// heap wrapper substrate: typeof / instanceof / Object.prototype.
// toString.call all report the wrapper's Object identity, not the
// coerced primitive.

const s = new String("hi");
console.log("typeof:", typeof s);
console.log("instanceof:", s instanceof String);
console.log("toStringTag:", Object.prototype.toString.call(s));
