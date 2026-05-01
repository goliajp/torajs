// Adapted from test262: built-ins/Array/prototype/sort/* — in-place sort
// using the supplied comparator. Subset requires the comparator (no
// default lex-sort fallback). ssa-lower emits inline insertion sort
// (O(n²) but works for moderate arrays); each compare goes through
// the closure dispatch (call_fn_value).
//
// Comparator return: i64 (ICmp > 0) or f64 (FCmp > 0) handled at
// lower-time based on the closure's actual return type.
function check(): number {
  // Number ascending.
  let xs: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
  xs.sort((a: number, b: number): number => a - b);
  if (xs[0] !== 1) { throw "#1"; }
  if (xs[1] !== 1) { throw "#2"; }
  if (xs[2] !== 2) { throw "#3"; }
  if (xs[7] !== 9) { throw "#4"; }

  // Number descending.
  let ys: number[] = [3, 1, 4, 1, 5];
  ys.sort((a: number, b: number): number => b - a);
  if (ys[0] !== 5) { throw "#5"; }
  if (ys[4] !== 1) { throw "#6"; }

  // Already sorted — stable output.
  let sorted: number[] = [1, 2, 3, 4];
  sorted.sort((a: number, b: number): number => a - b);
  if (sorted[0] !== 1) { throw "#7"; }
  if (sorted[3] !== 4) { throw "#8"; }

  // Reverse-sorted — full reorder.
  let rev: number[] = [5, 4, 3, 2, 1];
  rev.sort((a: number, b: number): number => a - b);
  if (rev[0] !== 1) { throw "#9"; }
  if (rev[4] !== 5) { throw "#10"; }

  // Single element + empty.
  let one: number[] = [42];
  one.sort((a: number, b: number): number => a - b);
  if (one[0] !== 42) { throw "#11"; }
  let empty: number[] = [];
  empty.sort((a: number, b: number): number => a - b);
  if (empty.length !== 0) { throw "#12"; }

  // Negative + positive mix.
  let mix: number[] = [-3, 5, -1, 0, 2, -7];
  mix.sort((a: number, b: number): number => a - b);
  if (mix[0] !== -7) { throw "#13"; }
  if (mix[5] !== 5) { throw "#14"; }

  // String[] sort by length.
  let words: string[] = ["zzz", "a", "bbbb", "cc"];
  words.sort((a: string, b: string): number => a.length - b.length);
  if (words[0] !== "a") { throw "#15"; }
  if (words[3] !== "bbbb") { throw "#16"; }
  return 0;
}
console.log(check());
