// RFC 20260707 chunk 621 — a typed array shared into an `Arr<Any>`
// slot (T-11 container widen) keeps its raw-slot layout; the static
// any[] readers rebox per the elem kind recorded at the coercion
// boundary. Pre-fix: i64 slots NaN-box-misread (undefined/null),
// f64 decoded shifted (1.5 -> 1.0625), bool SIGSEGV'd, for-of crashed.
class Box {
  arr: any[] = [];
}

// i64 elems through a class field
const b = new Box();
const nums: number[] = [10, 20, 30];
b.arr = nums;
console.log(b.arr.length);
console.log(b.arr[0]);
console.log(b.arr[2]);
for (const x of b.arr) console.log(x);

// f64 elems
const fb = new Box();
const fs: number[] = [1.5, 2.5, 3.5];
fb.arr = fs;
console.log(fb.arr[0]);
console.log(fb.arr[2]);

// bool elems
const bb = new Box();
const bs: boolean[] = [true, false, true];
bb.arr = bs;
console.log(bb.arr[0]);
console.log(bb.arr[1]);

// heap elems keep working, and aliasing stays live: a push through
// the typed view is visible through the any view
const sb = new Box();
const strs: string[] = ["a", "bb"];
sb.arr = strs;
strs.push("ccc");
console.log(sb.arr.length);
console.log(sb.arr[2]);

// local let binding form of the same widen
function g(): void {
  const local: number[] = [7, 8, 9];
  const xs: any[] = local;
  console.log(xs.length);
  console.log(xs[0]);
  console.log(xs[2]);
}
g();
