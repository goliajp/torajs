// Rotation 204 — §B.3.1 step 5: the `__proto__: v` [[Prototype]]-set
// special case requires IsComputedPropertyKey(propKey) = false. A
// computed `['__proto__']` key defines an ordinary OWN property (the
// parser's fold to a plain field name now records it in the same
// own-property side channel the `{ __proto__ }` shorthand uses).

let sample = { s: 1 };

// computed key — own property, prototype unchanged
let obj = { ["__proto__"]: sample };
console.log(Object.getPrototypeOf(obj) === sample);
console.log(obj.hasOwnProperty("__proto__"));

let obj2 = { ["__proto__"]: null };
console.log(Object.getPrototypeOf(obj2) === null);
console.log(obj2.hasOwnProperty("__proto__"));

// shorthand — own property (regression guard for the shared channel)
let __proto__ = sample;
let sh = { __proto__ };
console.log(sh.hasOwnProperty("__proto__"));
