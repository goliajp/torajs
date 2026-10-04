// A popcount sum over a counted loop collapses into one range-sum
// instruction. Its accumulator check may be dropped only on a bound
// the loop really has: the counter's run-time bound here comes from
// argv, and the sums below cross 2^53 where the bound is large, so
// they must round as f64 sums do.

function popcount(x: number): number {
  let c = 0;
  while (x !== 0) {
    x = x & (x - 1);
    c = c + 1;
  }
  return c;
}

const n = parseInt(process.argv[2] ?? "100000", 10) || 100000;

let total = 0;
for (let i = 0; i < n; i++) {
  total = total + popcount(i);
}
console.log(total);

// the same sum from a start past 2^31: popcount works on int32, so
// these counts are of the low 32 bits
const base = parseInt("4294967296", 10);
let t2 = 0;
for (let i = 0; i < 1000; i++) {
  t2 = t2 + popcount(base + i);
}
console.log(t2);

// an accumulator that starts near 2^53 leaves the exact range
let t3 = parseInt("9007199254740000", 10);
for (let i = 0; i < 4096; i++) {
  t3 = t3 + popcount(i);
}
console.log(t3, t3 > 9007199254740991);
