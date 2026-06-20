// ES §19.4.{2,3} — Symbol.{for,keyFor} trailing-arg ignore.
// Spec reads only args[0]; tora silent-drops trailing.

const s1 = Symbol.for("k");
const s2 = Symbol.for("k", "extra");
const s3 = Symbol.for("k", 99, "ignored");

// Identity preserved across all 3 calls (same global registry).
console.log(typeof s1);
console.log(s1 === s2);
console.log(s2 === s3);

// keyFor — trailing arg dropped.
console.log(Symbol.keyFor(s1));
console.log(Symbol.keyFor(s1, "extra"));
console.log(Symbol.keyFor(s2, 1, 2, 3));
