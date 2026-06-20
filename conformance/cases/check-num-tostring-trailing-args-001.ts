// S244 — Number.toString(radix, ...trailing) trailing-arg ignore
// per ES §21.1.3.6. Spec reserves slots past the 1 useful radix
// but tora's helpers (num_to_string_radix_i / num_to_string_radix_f)
// are 1-arg only. Trailing operand type_of'd for side effects then
// dropped at lower-time. Same shape as S243 Math.* trailing-arg
// ignore.

const n: number = 16;
console.log(n.toString());
console.log(n.toString(10));
console.log(n.toString(10, 99));
console.log(n.toString(16, 99));
console.log(n.toString(16, "junk" as any));
console.log(n.toString(undefined, 99));
console.log(n.toString(undefined, "junk" as any));

console.log((255).toString(16, 99));
console.log((255).toString(2, "junk" as any));
console.log((1.5).toString(10, 99));
console.log((1.5).toString(2, 99));
