// RFC C4 — Object.getOwnPropertyDescriptor argument validation. Spec
// §20.1.2.8 step 1 `Let obj be ? ToObject(O)`: ToObject on undefined /
// null throws a TypeError; every other primitive (number / string /
// boolean) boxes to a wrapper, so a missing key yields undefined
// (bun parity). Assertion shape uses a `threw` flag (mirrors the RFC
// C3 setter fixture) — `e.constructor.name === "TypeError"` requires a
// program-level `TypeError` reference to register the slot factory,
// which is orthogonal to C4 and a v0 limitation.

let threw_undef = false;
try {
  Object.getOwnPropertyDescriptor(undefined, "x");
} catch (e) {
  threw_undef = true;
}
console.log(threw_undef); // true

let threw_null = false;
try {
  Object.getOwnPropertyDescriptor(null, "x");
} catch (e) {
  threw_null = true;
}
console.log(threw_null); // true

// Number primitive — ToObject boxes to a Number wrapper, no own "x".
console.log(Object.getOwnPropertyDescriptor(42, "x")); // undefined

// Any-typed receiver holding undefined — runtime tag discrimination.
const o: any = undefined;
let threw_any = false;
try {
  Object.getOwnPropertyDescriptor(o, "x");
} catch (e) {
  threw_any = true;
}
console.log(threw_any); // true
