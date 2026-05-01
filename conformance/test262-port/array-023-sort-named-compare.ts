// Adapted from test262 array.prototype.sort/* — exercises sort with a
// named comparator (top-level fn). Anonymous-arrow comparators don't
// yet inference-promote the return type in tr, so until that lands the
// idiomatic shape here is to hoist the comparator. Default-string sort
// (no comparator) covered separately.
function ncmp(a: number, b: number): number { return a - b; }
function rev_ncmp(a: number, b: number): number { return b - a; }

function check(): number {
  let arr = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
  arr.sort(ncmp);
  // Expected ascending order.
  let exp = [1, 1, 2, 3, 3, 4, 5, 5, 5, 6, 9];
  if (arr.length !== exp.length) { throw "#1: length"; }
  for (let i = 0; i < arr.length; i++) {
    if (arr[i] !== exp[i]) { throw "#2: ascending"; }
  }

  // Reverse comparator.
  let arr2 = [1, 5, 3, 7, 2];
  arr2.sort(rev_ncmp);
  let exp2 = [7, 5, 3, 2, 1];
  for (let i = 0; i < arr2.length; i++) {
    if (arr2[i] !== exp2[i]) { throw "#3: descending"; }
  }

  // Empty array sorts to empty.
  let empty: number[] = [];
  empty.sort(ncmp);
  if (empty.length !== 0) { throw "#4: empty"; }

  // Single-element sort is a no-op.
  let one = [42];
  one.sort(ncmp);
  if (one[0] !== 42) { throw "#5: single"; }

  return 0;
}
console.log(check());
