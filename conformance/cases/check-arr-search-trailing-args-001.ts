// S239 — Array<T>.{indexOf,lastIndexOf,includes}(needle, fromIndex,
// ...trailing) trailing-arg ignore per ES §23.1.3.{14,17,18}: spec
// reserves slots after fromIndex but tora's inline scan is 2-arg
// only. Trailing operands type_of'd for side effects then dropped
// at lower-time (args[2..] never SSA-emitted). Same shape as S238
// localeCompare.

const xs: number[] = [1, 2, 3, 2, 1];
console.log(xs.indexOf(2, 0, 99));
console.log(xs.indexOf(2, 2, "junk" as any));
console.log(xs.lastIndexOf(2, 4, 99));
console.log(xs.lastIndexOf(2, 1, "junk" as any));
console.log(xs.includes(2, 0, 99));
console.log(xs.includes(99, 0, "junk" as any));

const ss: string[] = ["a", "b", "c", "b", "a"];
console.log(ss.indexOf("b", 0, 99));
console.log(ss.lastIndexOf("b", 4, 99));
console.log(ss.includes("b", 0, 99));
