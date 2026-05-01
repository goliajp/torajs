// Adapted from test262: language/expressions/array/spread/* — array
// spread `[...xs, lit, ...ys]`. tr's lowering pre-computes total length
// (literal count + sum of spread sources' .length), allocs once with
// that capacity, fills via arr_push_unchecked + a single memcpy per
// spread (__torajs_arr_extend_unchecked). No realloc.
function check(): number {
  let xs: number[] = [1, 2, 3];
  let ys: number[] = [10, 20];

  let combined = [...xs, 100, ...ys];
  if (combined.length !== 6) { throw "#1"; }
  if (combined[0] !== 1) { throw "#2"; }
  if (combined[2] !== 3) { throw "#3"; }
  if (combined[3] !== 100) { throw "#4"; }
  if (combined[4] !== 10) { throw "#5"; }
  if (combined[5] !== 20) { throw "#6"; }

  let copy = [...xs];
  if (copy.length !== 3) { throw "#7"; }
  if (copy[1] !== 2) { throw "#8"; }

  let nested = [...xs, ...xs];
  if (nested.length !== 6) { throw "#9"; }
  if (nested[3] !== 1) { throw "#10"; }
  if (nested[5] !== 3) { throw "#11"; }

  // Empty source spread.
  let empty: number[] = [];
  let from_empty = [42, ...empty, 99];
  if (from_empty.length !== 2) { throw "#12"; }
  if (from_empty[0] !== 42) { throw "#13"; }
  if (from_empty[1] !== 99) { throw "#14"; }
  return 0;
}
console.log(check());
