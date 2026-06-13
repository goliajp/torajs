// RFC C4b — Object.defineProperty argument validation. Spec ES §10.1.6.3
// step 1 `If Type(O) is not Object, throw a TypeError`. Strict Type(O)
// check (unlike gOPD's ToObject semantics): every primitive throws,
// including `string` / `number` / `boolean`. Real objects pass through.
// Assertion shape uses a `threw` flag (mirrors the RFC C3/C4a fixtures) —
// `e.constructor.name === "TypeError"` requires a program-level
// `TypeError` reference to register the slot factory, orthogonal to C4b.

let threw_undef = false;
try {
  Object.defineProperty(undefined, "x", { value: 1 });
} catch (e) {
  threw_undef = true;
}
console.log(threw_undef); // true

let threw_null = false;
try {
  Object.defineProperty(null, "x", { value: 1 });
} catch (e) {
  threw_null = true;
}
console.log(threw_null); // true

let threw_num = false;
try {
  Object.defineProperty(42, "x", { value: 1 });
} catch (e) {
  threw_num = true;
}
console.log(threw_num); // true

let threw_bool = false;
try {
  Object.defineProperty(true, "x", { value: 1 });
} catch (e) {
  threw_bool = true;
}
console.log(threw_bool); // true

let threw_str = false;
try {
  Object.defineProperty("s", "x", { value: 1 });
} catch (e) {
  threw_str = true;
}
console.log(threw_str); // true

// Any-typed receiver holding undefined — runtime tag discrimination.
const o: any = undefined;
let threw_any = false;
try {
  Object.defineProperty(o, "x", { value: 1 });
} catch (e) {
  threw_any = true;
}
console.log(threw_any); // true

// Sanity: a real object accepts the call without throwing.
const target: any = {};
Object.defineProperty(target, "x", { value: 7 });
console.log(target.x); // 7
