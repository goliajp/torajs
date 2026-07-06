// RFC 20260707 chunk 625 — inline-emitted consumers (HOF loops,
// sort comparisons, find family, flatMap) read Any elems through the
// kind-aware borrowed-box helper instead of a raw LoadDyn, so typed
// blocks behind Arr<Any> views work; flatMap's Any dst allocates the
// FLAG_ARR_ANY flavor and a returned array literal from an
// Arr<Any>-ret fn takes the annotation widen (was an unmarked typed
// block reading as all-undefined).
class Box {
  arr: any[] = [];
}
const b = new Box();
const nums: number[] = [30, 10, 20];
b.arr = nums;

// HOF family over the typed-behind-any view
console.log(b.arr.map((x: any) => x * 2)[0]);
console.log(b.arr.filter((x: any) => x > 15).length);
console.log(b.arr.reduce((a: any, x: any) => a + x, 0));
console.log(b.arr.find((x: any) => x > 15));
console.log(b.arr.findIndex((x: any) => x === 10));
console.log(b.arr.some((x: any) => x > 25));
console.log(b.arr.every((x: any) => x > 5));
b.arr.forEach((x: any) => console.log(x));

// sort mutates in place; the typed alias observes it
b.arr.sort();
console.log(b.arr[0]);
console.log(nums[0]);

// flatMap: typed-behind-any src + Arr<Any>-ret literal cb
const fm = b.arr.flatMap((x: any) => [x, x]);
console.log(fm.length);
console.log(fm[1]);
console.log(fm[5]);

// plain any[] flatMap keeps working
const xs: any[] = [1, "a"];
const fm2 = xs.flatMap((x: any) => [x, x]);
console.log(fm2[0]);
console.log(fm2[3]);
