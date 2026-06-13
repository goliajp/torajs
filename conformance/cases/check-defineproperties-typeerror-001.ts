// RFC C4b — Object.defineProperties argument validation. Spec ES §20.1.2.4
// `Object.defineProperties(O, Properties)` step 1 = `If Type(O) is not
// Object, throw a TypeError`. Compile-time unfold expands to one
// `defineProperty` call per descriptor field; the receiver guard fires
// on the first iteration for any non-object O.

let threw_undef = false;
try {
  Object.defineProperties(undefined, { x: { value: 1 } });
} catch (e) {
  threw_undef = true;
}
console.log(threw_undef); // true

let threw_null = false;
try {
  Object.defineProperties(null, { x: { value: 1 } });
} catch (e) {
  threw_null = true;
}
console.log(threw_null); // true

let threw_num = false;
try {
  Object.defineProperties(42, { x: { value: 1 } });
} catch (e) {
  threw_num = true;
}
console.log(threw_num); // true

// Sanity: a real object accepts the call without throwing.
const target: any = {};
Object.defineProperties(target, { x: { value: 7 }, y: { value: 9 } });
console.log(target.x); // 7
console.log(target.y); // 9
