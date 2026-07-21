// RFC 20260721-array-proto-cluster 刀 13b — toReversed on an
// accessor/exotic receiver rides per-index [[Get]] via the
// arr_any_slice gather (§23.1.3.33 step 5): the accessor getter
// runs, a mid-gather length shrink reads later indexes through the
// prototype digit keys, and absent indexes answer undefined.
const arr = [0, 1, 2, 3, 4];
(Array.prototype as any)[1] = 5;
Object.defineProperty(arr, "3", {
  get() {
    arr.length = 1;
    return 3;
  },
});
console.log(arr.toReversed());
delete (Array.prototype as any)["1"];
// clean receiver keeps the raw-copy fast path
console.log([7, 8, 9].toReversed());
