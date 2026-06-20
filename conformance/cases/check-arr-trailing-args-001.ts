// S242 — Array<T>.{at,slice,join}(useful, ...trailing) trailing-arg
// ignore per ES §23.1.3.{1,28,16}. Spec reserves slots past the
// useful args but tora's helpers are 1-/2-/1-arg only; trailing
// operand type_of'd for side effects then dropped at lower-time.
// Same shape as S238/S239/S240/S241.

const xs: number[] = [10, 20, 30, 40, 50];
console.log(xs.at(1, 99));
console.log(xs.at(-1, "junk" as any));
console.log(xs.at(undefined, 99));

console.log(xs.slice(0, 3, 99));
console.log(xs.slice(1, 4, "junk" as any));
console.log(xs.slice(undefined, undefined, 99));
console.log(xs.slice(-2, undefined, 99));

console.log(xs.join("-", 99));
console.log(xs.join(",", "junk" as any));
console.log(xs.join(undefined, 99));

const ss: string[] = ["a", "b", "c"];
console.log(ss.at(0, 99));
console.log(ss.slice(0, 2, 99));
console.log(ss.join("/", 99));

const bs: boolean[] = [true, false, true];
console.log(bs.join(",", 99));
