// RFC 20260707 chunk 622 — writes through a static Arr<Any> view of
// a typed array kind-coerce into the raw slots (pre-fix the NaN-box
// bits were stored raw: reads returned NaN / huge integers). The
// typed alias observes every write.
class Box {
  arr: any[] = [];
}

// i64 elems: index write + append-by-index + push
const b = new Box();
const nums: number[] = [10, 20, 30];
b.arr = nums;
b.arr[0] = 42;
console.log(b.arr[0]);
console.log(nums[0]);
b.arr[3] = 99;
console.log(nums.length);
console.log(nums[3]);
b.arr.push(77);
console.log(b.arr.length);
console.log(nums[4]);

// f64 elems: index write keeps double bits; int widens
const fb = new Box();
const fs: number[] = [1.5, 2.5];
fb.arr = fs;
fb.arr[1] = 9.25;
console.log(fs[1]);

// bool elems
const bb = new Box();
const bs: boolean[] = [true, false];
bb.arr = bs;
bb.arr[0] = false;
console.log(bs[0]);

// fill through the view, typed alias sees it
const xb = new Box();
const xs: number[] = [1, 2, 3, 4];
xb.arr = xs;
xb.arr.fill(7, 1, 3);
console.log(xs[0]);
console.log(xs[1]);
console.log(xs[2]);
console.log(xs[3]);

// heap elems: push a string through the view
const sb = new Box();
const strs: string[] = ["a", "bb"];
sb.arr = strs;
sb.arr.push("ccc");
console.log(strs.length);
console.log(strs[2]);
