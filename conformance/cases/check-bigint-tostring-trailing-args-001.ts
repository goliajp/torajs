// S247 — BigInt.prototype.toString(radix, ...trailing) trailing-
// arg ignore per ES §21.2.3.5. Spec reserves slots past the 1
// useful radix but tora's helpers (bigint_to_string /
// bigint_to_string_radix) are 1-arg only; trailing operand
// type_of'd for side effects then dropped at lower-time. Same
// shape as S244 Number.toString trailing-arg ignore.

console.log(255n.toString());
console.log(255n.toString(16));
console.log(255n.toString(16, 99));
console.log(255n.toString(16, "junk" as any));
console.log(255n.toString(2, 99));
console.log(255n.toString(undefined));
console.log(255n.toString(undefined, 99));
console.log(255n.toString(undefined, "junk" as any));

console.log((-1n).toString(16, 99));
console.log(0n.toString(36, 99));
