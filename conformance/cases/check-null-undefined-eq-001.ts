// ES §7.2.13 IsLooselyEqual step 2 — `null == undefined` and
// `undefined == null` are true; loose-eq with any other type
// short-circuits to false without coercion (Null/Undefined never
// coerces to Number/String per spec).
//
// ES §7.2.15 IsStrictlyEqual — `null === undefined` is false
// (Null and Undefined are distinct primitive types per §6.1.1
// / §6.1.2).
//
// Pre-fix tr rejected the literal pair at check time with
// "strict equality requires same primitive type, got Null and
// Undefined" — every `x == null` style nullish check that
// reached the test author's hands as `x == undefined` was
// type-rejected too.
console.log(null == undefined);       // true
console.log(undefined == null);       // true
console.log(null === undefined);      // false
console.log(undefined === null);      // false
console.log(null != undefined);       // false
console.log(null !== undefined);      // true

// Null-to-Null and Undefined-to-Undefined remain spec-true
console.log(null == null);            // true
console.log(undefined == undefined);  // true
console.log(null === null);           // true
console.log(undefined === undefined); // true
