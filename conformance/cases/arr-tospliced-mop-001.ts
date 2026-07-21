// RFC 20260721-array-proto-cluster 刀 13c — toSpliced on an
// accessor/exotic receiver rides per-index [[Get]] via the
// arr_any_slice gather (§23.1.3.42 steps 16/18): accessor getters
// run in index order and observe each other's mutations, a
// mid-gather length shrink reads through the prototype digit keys,
// and the splice then operates on the clean dense clone.

// length decreased while iterating (proto digit key backfills)
{
  const arr = [0, 1, 2, 3, 4, 5];
  (Array.prototype as any)[3] = 6;
  Object.defineProperty(arr, "2", {
    get() {
      arr.length = 1;
      return 2;
    },
  });
  console.log(arr.toSpliced(0, 0));
  delete (Array.prototype as any)["3"];
}
// length increased while iterating (len snapshot holds)
{
  const arr = [0, 1, 2];
  Object.defineProperty(arr, "0", {
    get() {
      arr.push(10);
      return 0;
    },
  });
  Object.defineProperty(arr, "2", {
    get() {
      arr.push(11);
      return 2;
    },
  });
  console.log(arr.toSpliced(1, 0, 0.5));
}
// getters mutate mid-copy; later reads observe the writes
{
  const arr = [0, 1, 2, 3];
  let z = arr[0];
  Object.defineProperty(arr, "0", {
    get() {
      arr[1] = 42;
      return z;
    },
    set(v) {
      z = v;
    },
  });
  Object.defineProperty(arr, "2", {
    get() {
      arr[0] = 17;
      arr[3] = 37;
      return 2;
    },
  });
  console.log(arr.toSpliced(1, 0, 0.5));
}
// clean receiver keeps the raw-memcpy fast path
console.log([1, 2, 3].toSpliced(1, 1, 9));
