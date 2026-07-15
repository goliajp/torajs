// RFC 20260716-primitive-wrapper-substrate 刀 2c — `new Boolean(x)`
// heap wrapper substrate: typeof / instanceof / Object.prototype.
// toString.call all report the wrapper's Object identity, not the
// coerced primitive.

const b = new Boolean(false);
console.log("typeof:", typeof b);
console.log("instanceof:", b instanceof Boolean);
console.log("toStringTag:", Object.prototype.toString.call(b));
