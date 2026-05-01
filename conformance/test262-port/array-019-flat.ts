// Adapted from test262: built-ins/Array/prototype/flat/* — single-level
// array flattening. Receiver `T[][]` → result `T[]`. Each inner array
// is concatenated end-to-end via two-pass sum-then-memcpy in the
// runtime. v0 supports depth=1 only (no `.flat(2)` arg, no Infinity).
function check(): number {
  // Basic flatten.
  let nested: number[][] = [[1, 2], [3], [4, 5, 6]];
  let flat = nested.flat();
  if (flat.length !== 6) { throw "#1"; }
  if (flat[0] !== 1) { throw "#2"; }
  if (flat[2] !== 3) { throw "#3"; }
  if (flat[5] !== 6) { throw "#4"; }

  // Empty outer.
  let outer_empty: number[][] = [];
  let f4 = outer_empty.flat();
  if (f4.length !== 0) { throw "#5"; }

  // Single inner array.
  let single: number[][] = [[1, 2, 3]];
  let f5 = single.flat();
  if (f5.length !== 3) { throw "#6"; }
  if (f5[1] !== 2) { throw "#7"; }

  // String[][] flatten.
  let words: string[][] = [["alpha", "beta"], ["gamma"]];
  let fw = words.flat();
  if (fw.length !== 3) { throw "#8"; }
  if (fw[0] !== "alpha") { throw "#9"; }
  if (fw[2] !== "gamma") { throw "#10"; }

  // Original is unchanged.
  if (nested.length !== 3) { throw "#11: original unchanged"; }
  return 0;
}
console.log(check());
