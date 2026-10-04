// A fill loop carries two counters that step together: the program's
// own `i`, and the array's running length that addresses each slot.
// The length can be read as `i` plus the length the loop started from,
// so the loop keeps only `i`. Every append must still land at the
// array's length, and the length after the loop must be the real one,
// whatever the bound is and wherever the array started.

function fill(start: number[], n: number): number[] {
  const xs: number[] = start.slice();
  const ys: number[] = [];
  for (let i = 0; i < n; i++) {
    ys.push(i);
  }
  for (let i = 0; i < ys.length; i++) {
    xs.push(ys[i]);
  }
  return xs;
}

const n = parseInt(process.argv[2] ?? "9", 10) || 9;

// started non-empty, so the length runs ahead of the counter
const a: number[] = [40, 41, 42, 43];
for (let i = 0; i < n; i++) {
  a.push(i * 2);
}
console.log(a.length, a[3], a[4], a[a.length - 1], a.join(","));

// bounds that are not whole, not numbers, or not positive
for (const bound of [2.5, 0.1, -4, NaN, 0, 3]) {
  const b: number[] = [7];
  for (let i = 0; i < bound; i++) {
    b.push(i);
  }
  console.log(bound, b.length, b.join(","));
}

// the length after the loop feeds the next fill of the same array
const c: number[] = [];
for (let i = 0; i < n; i++) {
  c.push(i);
}
const mid = c.length;
for (let i = 0; i < n; i++) {
  c.push(mid + i);
}
let total = 0;
for (let i = 0; i < c.length; i++) {
  total = total + c[i];
}
console.log(mid, c.length, total, c[mid - 1], c[mid], c[c.length - 1]);

// a copy into a non-empty array, through a function
console.log(fill([1, 2], 5).join(","), fill([], 0).length);

// a while loop over its own counter
const d: number[] = [9, 9];
let k = 0;
while (k < n) {
  d.push(k + 100);
  k = k + 1;
}
console.log(k, d.length, d[2], d[d.length - 1]);
