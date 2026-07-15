// RFC 20260716-primitive-wrapper-substrate 刀 2a — `new Number(x)`
// heap wrapper substrate: typeof / instanceof / Object.prototype.
// toString.call all report the wrapper's Object identity, not the
// coerced primitive.

const n = new Number(1);
console.log("typeof:", typeof n);
console.log("instanceof:", n instanceof Number);
console.log("toStringTag:", Object.prototype.toString.call(n));
