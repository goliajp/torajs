// The anchor and a later element differing in any-ness at depth 0 is
// the same repr disagreement rotation 231 already recognised one
// level INSIDE a container, and it went unasked at the top. Lowering
// has always built these literals in the any lane, so the checker's
// `Array<Number>` was fiction and every consumer generated against it
// read a 16-byte tagged slot through an 8-byte scalar contract.

const zero: any = 0;
const one: any = 1;
const two: any = 2;

// Nothing in this literal is a spread and only one item is `any`.
const mixed = [1, zero];
console.log("ab".substr(mixed[1], 1));
for (const v of mixed) {
  console.log("ab".substr(v, 1));
}

const withOne = [0, one];
console.log("abc".charAt(withOne[1]));
console.log("abc".at(withOne[1]));
console.log(withOne[1] + 1);

const withTwo = [0, two];
console.log("ab".repeat(withTwo[1]));

const table = [10, 20, 30];
console.log(table[withOne[1]]);

// A spread of an any-array behind a numeric anchor is the same shape.
const anys: any[] = [0];
console.log("ab".substr([1, ...anys][1], 1));

// The test262 substr matrix that found this: a numeric array spread
// beside the any-typed product of mapping over it.
const p = [0, 1];
const integers = [...p, ...p.map(v => -v)];
const numbers = [...integers, ...integers.map(v => v + 0.5)];
for (const start of numbers) {
  console.log(start, "=>", "a".substr(start, 1));
}

// Homogeneous literals keep their typed slots — numbers stay one
// bucket, and an `any` anchor was already widening correctly.
const ints = [1, 2, 3];
let sum = 0;
for (const v of ints) sum += v;
console.log(sum, ints.length);
console.log([...[1, 2], ...[3.5]][2]);
const anchored = [one, 2, "z"];
console.log(anchored[0], anchored[1], anchored[2]);
console.log([[1], [one]][1][0] + 1);
