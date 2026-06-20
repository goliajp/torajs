// S241 — String.{slice,substring,substr,padStart,padEnd}(a, b,
// ...trailing) trailing-arg ignore per ES §22.1.3.{20,22,23,16,17}.
// Spec reserves slots past the 2 useful args but tora's helpers are
// 2-arg only; trailing operand type_of'd for side effects then
// dropped at lower-time (ssa_lower break early past i=1). Same
// shape as S238/S239/S240.

console.log("abcdef".slice(0, 3, 99));
console.log("abcdef".slice(0, 3, "junk" as any));
console.log("abcdef".slice(-2, undefined, 99));
console.log("abcdef".substring(1, 4, 99));
console.log("abcdef".substring(1, 4, "junk" as any));
console.log("abcdef".substring(undefined, undefined, 99));
console.log("abcdef".substr(0, 3, 99));
console.log("abcdef".substr(0, 3, "junk" as any));

console.log("5".padStart(3, "0", 99));
console.log("5".padStart(3, "0", "junk" as any));
console.log("5".padStart(3, undefined, 99));
console.log("5".padEnd(3, "0", 99));
console.log("5".padEnd(3, "0", "junk" as any));
console.log("5".padEnd(3, undefined, 99));
