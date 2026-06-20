// S239 — String.{indexOf,lastIndexOf,includes,startsWith,endsWith}
// (needle, fromIndex, ...trailing) trailing-arg ignore per ES
// §22.1.3.{8,10,5,21,7}: spec reserves slots after fromIndex but
// tora's helpers are 2-arg only. Trailing operands type_of'd for
// side effects then dropped at lower-time (ssa_lower break early
// past i=1). Same shape as S238 localeCompare.

const s = "hello world";
console.log(s.indexOf("o", 0, 99));
console.log(s.indexOf("o", 5, "junk" as any));
console.log(s.lastIndexOf("o", 10, 99));
console.log(s.lastIndexOf("o", 4, "junk" as any));
console.log(s.includes("o", 0, 99));
console.log(s.includes("z", 0, 99));
console.log(s.startsWith("h", 0, 99));
console.log(s.startsWith("e", 1, 99));
console.log(s.endsWith("d", 11, 99));
console.log(s.endsWith("o", 5, 99));
