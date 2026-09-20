// ES §20.1.2.12 step 1 is ToObject(obj), so a primitive is not "no
// prototype available" -- it gets its wrapper, and the wrapper's
// [[Prototype]] is the matching %C.prototype%.
//
// Number and Boolean are the only primitives that are not refcounted,
// so they were the only two that missed the heap-receiver arm and fell
// into a fallthrough that answered ANY_NULL. That made
// `Object.getPrototypeOf(0) === Number.prototype` answer FALSE -- a
// wrong answer rather than a refusal, which is why it sat in the bugs
// bucket instead of the incompatible one and no cluster pass looked at
// it.
console.log(Object.getPrototypeOf(0) === Number.prototype);
console.log(Object.getPrototypeOf(1.5) === Number.prototype);
console.log(Object.getPrototypeOf(true) === Boolean.prototype);
console.log(Object.getPrototypeOf(false) === Boolean.prototype);

// already correct through the heap-receiver arm -- must not move
console.log(Object.getPrototypeOf("s") === String.prototype);
console.log(Object.getPrototypeOf(1n) === BigInt.prototype);
console.log(Object.getPrototypeOf([]) === Array.prototype);

// nullish still throws: ToObject(null) / ToObject(undefined) is a
// TypeError, and that gate runs before the primitive arm.
try { Object.getPrototypeOf(null); console.log("no-throw"); } catch (e) { console.log("null threw"); }
try { Object.getPrototypeOf(undefined); console.log("no-throw"); } catch (e) { console.log("undefined threw"); }

// The judgment this knife was found by, written down: a value spelled
// `any` and the same value left typed must get the same answer. The
// any lane was right all along; only the typed spelling said null.
const n: any = 0;
console.log(Object.getPrototypeOf(n) === Object.getPrototypeOf(0));
const b: any = true;
console.log(Object.getPrototypeOf(b) === Object.getPrototypeOf(true));
