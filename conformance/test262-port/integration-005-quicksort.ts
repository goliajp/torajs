// Integration: in-place-style quicksort, but emitting a fresh sorted
// array (no mutating swaps — tr's array slot is heap-owned and we
// don't yet have `arr[i] = x` write-back through arr_set runtime).
function partition(xs: number[], pivot: number): number[] {
  let out: number[] = [];
  for (let i: number = 0; i < xs.length; i = i + 1) {
    if (xs[i] < pivot) { out.push(xs[i]); }
  }
  out.push(pivot);
  for (let i: number = 0; i < xs.length; i = i + 1) {
    if (xs[i] > pivot) { out.push(xs[i]); }
  }
  return out;
}

function check(): number {
  let xs: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
  // Use `xs[0] = 3` as pivot. Result should sort around it.
  let sorted = partition(xs, 3);
  // sorted: [1, 1, 2, 3, 4, 5, 9, 6]
  if (sorted.length !== 8) { throw "#1"; }
  if (sorted[0] !== 1) { throw "#2"; }
  if (sorted[3] !== 3) { throw "#3"; }
  if (sorted[7] !== 6) { throw "#4"; }
  return 0;
}
console.log(check());
