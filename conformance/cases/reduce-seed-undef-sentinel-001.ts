// `xs.reduce(cb, seed)` hands the seed to the accumulator untouched
// (§23.1.3.24 step 6). A seed carrying the out-of-range sentinel was
// read as a plain number inside the callback, so `typeof acc` said
// "number" for an `undefined`.
const zs: number[] = [];

[1].reduce((acc, x) => {
  console.log("acc", typeof acc, acc, acc === undefined);
  return x;
}, zs[9]);

[1, 2].reduceRight((acc, x) => {
  console.log("right", typeof acc, acc === undefined);
  return x;
}, zs[9]);

// A live seed keeps the plain number lane, arithmetic and all.
const sum = [1, 2, 3].reduce((acc, x) => acc + x, 10);
console.log("live", typeof sum, sum);
