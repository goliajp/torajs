// P3.2-c.1 — Number predicates (isNaN/isFinite/isInteger/isSafeInteger)
console.log(Number.isNaN(NaN));         // true
console.log(Number.isNaN(0));            // false
console.log(Number.isNaN(1.5));          // false
console.log(Number.isNaN("NaN"));        // false (does NOT coerce)
console.log(Number.isFinite(0));         // true
console.log(Number.isFinite(Infinity));  // false
console.log(Number.isFinite(-Infinity)); // false
console.log(Number.isFinite(NaN));       // false
console.log(Number.isInteger(1));        // true
console.log(Number.isInteger(1.5));      // false
console.log(Number.isInteger(NaN));      // false
console.log(Number.isInteger(Infinity)); // false
console.log(Number.isSafeInteger(1));    // true
console.log(Number.isSafeInteger(9007199254740991));  // true (2^53-1)
console.log(Number.isSafeInteger(9007199254740992));  // false (2^53)
console.log(Number.isSafeInteger(1.5));  // false
console.log(Number.isSafeInteger(NaN));  // false
