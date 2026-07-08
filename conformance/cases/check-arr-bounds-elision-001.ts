// Guard-dominated bounds-check elision (ssa_lower_bounds_proven,
// RFC 20260708-typed-arr-oob-read perf follow-up) — semantics must
// be IDENTICAL with or without the elision. The `i < xs.length`
// guard re-proves every iteration, so `xs[i]` inside the untainted
// body window skips the OOB branch; a statement that writes the
// index evicts the pair, and every read after it keeps the checked
// lane (OOB → sentinel per the chunk 1-2 semantics).

// plain proven loop — direct loads, sum unchanged.
const xs: number[] = [1.5, 2.5, 3.5];
let sum: number = 0;
let i: number = 0;
while (i < xs.length) {
  sum = sum + xs[i];
  i = i + 1;
}
console.log(sum);                             // 7.5

// index write inside the body: reads AFTER the write stay checked —
// i jumps past the end, the checked lane answers the sentinel and
// the nullish default kicks in.
let hits: number = 0;
let j: number = 0;
while (j < xs.length) {
  j = j + 2;
  hits = hits + (xs[j] ?? 100);
}
console.log(hits);                            // 103.5 (3.5 + 100)

// for-loop shape — step writes after the body window, elision holds.
let s2: number = 0;
for (let k: number = 0; k < xs.length; k = k + 1) {
  s2 = s2 + xs[k];
}
console.log(s2);                              // 7.5

// array escape evicts: xs passed to a callee mid-body, later reads
// stay checked (still in-bounds here — semantics identical).
function len(a: number[]): number { return a.length; }
let s3: number = 0;
let m: number = 0;
while (m < xs.length) {
  s3 = s3 + len(xs);
  s3 = s3 + xs[m];
  m = m + 1;
}
console.log(s3);                              // 16.5 (3*3 + 7.5)
