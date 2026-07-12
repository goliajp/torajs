// RFC 20260713-array-proto-residual B1+B2 — derive-clone element
// descriptor propagation: a typed array whose alias class escapes to
// `any` (generic harness shape) stores NaN-box slots; every derive
// clone (toReversed / toSorted / toSpliced / with / slice / splice /
// concat / flat) must keep the product self-describing so element
// reads decode instead of answering undefined / crashing.
function esc<T>(x: T): void {
  const a: any = x;
  if (a === null) console.log("never");
}
let one = [1, 2, 3];
esc(one);
console.log(one.toReversed()[0], one.toReversed()[2]);
console.log(one.toSorted((a, b) => b - a)[0]);
console.log(one.toSpliced(1, 1)[1]);
console.log(one.with(1, 9)[0], one.with(1, 9)[1]);
console.log(one.slice(1)[0]);
console.log(one.concat([4])[3]);
let mut = [5, 6, 7];
esc(mut);
console.log(mut.splice(1, 1)[0], mut[1]);
let nested = [[1], [2]];
esc(nested);
console.log(nested.flat()[1]);
// ES2023 non-mutation contract survives the escaped class
let arr = [0, 1, 2];
esc(arr);
arr.with(1, 3);
console.log(arr[0], arr[1], arr[2]);
console.log(arr.with(1, 3) !== arr);
