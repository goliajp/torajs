// S246 — Array<T>.{copyWithin,fill}(a, b, c, ...trailing) trailing-
// arg ignore per ES §23.1.3.{4,7}. Spec reserves slots past the 3
// useful args but tora's inline copyWithin / fill loops are 3-arg
// only; trailing operand type_of'd for side effects then dropped
// at lower-time. Same shape as S243/S244/S245.

const xs: number[] = [1, 2, 3, 4, 5];
console.log(xs.copyWithin(0, 3, 4, 99));
console.log([1, 2, 3, 4, 5].copyWithin(0, 3, 4, "junk" as any));
console.log([1, 2, 3, 4, 5].copyWithin(-2, 0, 2, 99));

const ys: number[] = [1, 2, 3, 4, 5];
console.log(ys.fill(0, 1, 3, 99));
console.log([1, 2, 3, 4, 5].fill(7, 1, 3, "junk" as any));
console.log([1, 2, 3, 4, 5].fill(0, undefined, undefined, 99));

const ss: string[] = ["a", "b", "c", "d"];
console.log(ss.fill("X", 1, 3, 99));
