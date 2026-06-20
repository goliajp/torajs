// S255 — Object.keys / Object.getOwnPropertyNames / Reflect.ownKeys
// (obj, ...trailing) trailing-arg ignore per ES §20.1.2.{17,22} /
// §28.1.11. Spec reads only args[0]; trailing operands type_of'd then
// dropped at SSA-emit time.

const o = { a: 1, b: 2, c: 3 };

// --- Object.keys(obj, ...trailing) ---
console.log(Object.keys(o, 99));
console.log(Object.keys(o, 1, 2, 3));
console.log(Object.keys(o, "junk", true));

// --- Object.getOwnPropertyNames(obj, ...trailing) ---
console.log(Object.getOwnPropertyNames(o, 99));
console.log(Object.getOwnPropertyNames(o, true, false, "extra"));

// --- Reflect.ownKeys(obj, ...trailing) ---
console.log(Reflect.ownKeys(o, 99));
console.log(Reflect.ownKeys(o, 1, "junk", true));

// --- Array receiver ---
const a = [10, 20, 30];
console.log(Object.keys(a, 99));
console.log(Object.getOwnPropertyNames(a, "junk"));
