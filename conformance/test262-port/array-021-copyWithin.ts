// Adapted from test262: built-ins/Array/prototype/copyWithin/* — copies
// the [start, end) slice of receiver to position `target`, in-place.
// memmove handles overlapping ranges. All indices clamped to [0, len].
// Returns the same array.
function check(): number {
  // Basic non-overlapping copy.
  let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
  a.copyWithin(0, 3, 6);
  if (a[0] !== 4) { throw "#1"; }
  if (a[1] !== 5) { throw "#2"; }
  if (a[2] !== 6) { throw "#3"; }
  if (a[3] !== 4) { throw "#4: rest unchanged"; }

  // Overlapping forward (target < start).
  let b: number[] = [1, 2, 3, 4, 5];
  b.copyWithin(0, 1, 4);  // copy [2,3,4] to start → [2,3,4,4,5]
  if (b[0] !== 2) { throw "#5"; }
  if (b[1] !== 3) { throw "#6"; }
  if (b[2] !== 4) { throw "#7"; }
  if (b[3] !== 4) { throw "#8"; }

  // Overlapping backward (target > start).
  let c: number[] = [1, 2, 3, 4, 5];
  c.copyWithin(2, 0, 3);  // copy [1,2,3] to position 2 → [1,2,1,2,3]
  if (c[0] !== 1) { throw "#9"; }
  if (c[1] !== 2) { throw "#10"; }
  if (c[2] !== 1) { throw "#11"; }
  if (c[3] !== 2) { throw "#12"; }
  if (c[4] !== 3) { throw "#13"; }

  // Truncation when target+count > len.
  let d: number[] = [1, 2, 3, 4, 5];
  d.copyWithin(3, 0, 5);  // try copy 5 → only 2 fit → d = [1,2,3,1,2]
  if (d[3] !== 1) { throw "#14"; }
  if (d[4] !== 2) { throw "#15"; }

  // String[] case.
  let s: string[] = ["a", "b", "c", "d"];
  s.copyWithin(0, 2, 4);
  if (s[0] !== "c") { throw "#16"; }
  if (s[1] !== "d") { throw "#17"; }
  return 0;
}
console.log(check());
