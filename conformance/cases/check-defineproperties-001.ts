// RFC 20260613 C2 — Object.defineProperties(obj, { k: desc, ... }) with
// a literal descriptors object. Compile-time unfold: one defineProperty
// per field, re-reading the receiver each time so a resize in one field
// is seen by the next (10 fields force a dynobj resize past INITIAL_CAP).
const o: any = {};
Object.defineProperties(o, {
  a: { value: 1, enumerable: true, writable: false, configurable: true },
  b: { value: 2, enumerable: true, writable: true, configurable: false },
  c: { value: 3, enumerable: false },
  d: { value: 4, enumerable: true },
  e: { value: 5, enumerable: true },
  f: { value: 6, enumerable: true },
  g: { value: 7, enumerable: true },
  h: { value: 8, enumerable: true },
  i: { value: 9, enumerable: true },
  j: { value: 10, enumerable: true },
});
console.log(o.a, o.b, o.c, o.j);
const ga: any = Object.getOwnPropertyDescriptor(o, "a");
const gb: any = Object.getOwnPropertyDescriptor(o, "b");
const gc: any = Object.getOwnPropertyDescriptor(o, "c");
console.log(ga.writable, ga.enumerable, ga.configurable);
console.log(gb.writable, gb.enumerable, gb.configurable);
console.log(gc.enumerable);
let threw = false;
try {
  o.a = 99;
} catch (e) {
  threw = true;
}
console.log(threw, o.a);
