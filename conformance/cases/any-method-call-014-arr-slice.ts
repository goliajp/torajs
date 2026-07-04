// any-method-call RFC C4+ — arr.slice on any receivers, kind-aware
// over the block's self-description (FLAG_ARR_ANY 16-byte slots vs
// typed 8-byte slots + elem-kind bits; HEAP kinds rc_inc per slot).
const nums: number[] = [1, 2, 3, 4, 5];
const a: any = nums;
console.log(a.slice(1, 3));
console.log(a.slice(2));
console.log(a.slice(-2));
console.log(a.slice());
console.log(a.slice(3, 1));
console.log(a.slice(0, 99));
// source stays intact after slicing
console.log(a);
// string elements — HEAP kind slice keeps both arrays alive
const strs: string[] = ["x", "y", "z"];
const b: any = strs;
const tail: any = b.slice(1);
console.log(tail);
console.log(b);
console.log(tail[0]);
// float elements
const fs: number[] = [1.5, 2.5, 3.5];
const c: any = fs;
console.log(c.slice(0, 2));
// heterogeneous Arr<Any> receiver
const mix: any = [1, "two", true, 4.5];
console.log(mix.slice(1, 3));
console.log(mix.slice(-1));
// nested arrays survive the copy
const nested: any = [[1, 2], [3, 4], [5, 6]];
const mid: any = nested.slice(1, 2);
console.log(mid[0][1]);
// chained slice-of-slice
console.log(a.slice(1).slice(1, 2));
console.log("done");
