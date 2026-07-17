// Object/prototype/hasOwnProperty 8.12.1-1_20..22 — a struct-lane
// object literal's accessor member IS the own property under its
// public key. The compile-time fold only matched layout field
// names, so `{ get foo() {} }.hasOwnProperty("foo")` answered
// false (the accessor rides the layout as `__getter_foo`).

const o = { get foo() { return 42; } };
console.log(o.hasOwnProperty("foo")); // true
console.log(o.hasOwnProperty("bar")); // false
console.log(o.propertyIsEnumerable("foo")); // true

// runtime-key chain maps the synthetic names too
const k = "foo";
console.log(o.hasOwnProperty(k)); // true

// setter half
let sunk = 0;
const p = { set bar(v: number) { sunk = v; } };
console.log(p.hasOwnProperty("bar")); // true

// data fields unchanged
const d = { x: 1 };
console.log(d.hasOwnProperty("x"), d.hasOwnProperty("y")); // true false
console.log("done");
