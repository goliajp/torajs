// S245 — Array<T>.{reduce,reduceRight}(fn, init, ...trailing)
// trailing-arg ignore per ES §22.1.3.{21,22}. Spec reserves slots
// past the 2 useful args (callback + initial value) but tora's
// inline reduce loop is 2-arg only; trailing operand type_of'd
// for side effects then dropped at lower-time (SSA-emit reads
// only args[0..=1]). Same shape as S243/S244.

const xs: number[] = [1, 2, 3, 4, 5];
console.log(xs.reduce((a, b) => a + b, 0, 99));
console.log(xs.reduce((a, b) => a + b, 0, "junk" as any));
console.log(xs.reduce((a, b) => a * b, 1, 99));
console.log(xs.reduceRight((a, b) => a + b, 0, 99));
console.log(xs.reduceRight((a, b) => a - b, 0, "junk" as any));

const ss: string[] = ["a", "b", "c"];
console.log(ss.reduce((a, b) => a + b, "", 99));
console.log(ss.reduceRight((a, b) => a + b, "", 99));
