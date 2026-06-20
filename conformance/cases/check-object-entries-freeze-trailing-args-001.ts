// S256 — Object.{entries,freeze,isFrozen}(obj, ...trailing) trailing-
// arg ignore per ES §20.1.2.{5,12,15}. Spec reads only args[0]; trailing
// operands type_of'd then dropped at SSA-emit time.

const o = { a: 1, b: 2, c: 3 };

// --- Object.entries(obj, ...trailing) ---
console.log(Object.entries(o, 99).length);
console.log(Object.entries(o, 1, 2, 3).length);
console.log(Object.entries(o, "junk").length);

// --- Object.freeze(obj, ...trailing) ---
const f1 = { x: 1 };
const r1 = Object.freeze(f1, 99);
console.log(Object.isFrozen(r1, 99));

const f2 = { y: 2 };
Object.freeze(f2, 1, 2, 3);
console.log(Object.isFrozen(f2, true, false));

// --- Object.isFrozen(obj, ...trailing) ---
const f3 = { z: 3 };
console.log(Object.isFrozen(f3, 99));
Object.freeze(f3);
console.log(Object.isFrozen(f3, "junk", true));
