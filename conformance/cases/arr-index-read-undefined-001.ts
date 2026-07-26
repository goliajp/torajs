// ES §10.4.2.1 answers `undefined` for an absent index. A `string[]`
// read already did that; a `number[]` raised a RangeError instead,
// because tr stores an all-integral one in i64 slots and an i64 slot
// has no bit pattern to spare for that answer.
//
// Inside a named function it was worse than loud: the pending throw
// unwound the body, but the caller's throw-check is skipped for a
// callee that isn't known to throw — and an out-of-range read is an
// implicit throw with no `throw` node to find. The function stopped
// half-way, the caller read the ret sentinel, and the process exited 0.

const xs: number[] = [1, 2, 3];
console.log(xs[0], xs[2]);
console.log(xs[3]);
console.log(xs[99]);

// the string form has always answered this
const ss: string[] = ["a", "b"];
console.log(ss[1], ss[2]);

// through a named function — the shape that used to stop mid-body
function pick(src: number[], i: number): number {
  return src[i];
}
console.log(pick(xs, 1));
console.log(pick(xs, 7));

function sum_two(src: number[]): number {
  return src[0] + src[5];
}
console.log(sum_two(xs));

// `at` takes the same exit (§23.1.3.1 step 6) without being spelled
// `xs[i]`, so it seeds the element class the same way
console.log(xs.at(0), xs.at(2), xs.at(-1));
console.log(xs.at(3), xs.at(-9));

// widening costs no semantics — a JS number IS an f64, so the i64 form
// was the narrower one all along
const big: number[] = [9007199254740993, 2];
console.log(big[0], big[1]);
console.log(big[2]);

// fractional values still read back as themselves
const fs: number[] = [0.5, 1.5];
fs[0] = 2.25;
console.log(fs[0], fs[1], fs[4]);

// nested arrays answer undefined at the outer level too
const ns: number[][] = [[1, 2], [3]];
console.log(ns[0][1], ns[1][0]);
console.log(ns[1][1]);

// an empty array's every index is absent
const es: number[] = [];
console.log(es[0], es.at(0), es.length);
