// RFC 20260707 chunk 624 — join / concat / flat over typed blocks
// reached through Arr<Any> views. Pre-fix: join's ToString walk
// deref'd a bit-1-clear raw i64 (SIGSEGV); concat's typed seed took
// NaN-box splices raw (huge-integer reads); flat carried a typed
// inner array through as one scalar-looking element.
class Box {
  arr: any[] = [];
}

// join over i64 / f64 / bool / string elems
const b = new Box();
const nums: number[] = [30, 10, 20];
b.arr = nums;
console.log(b.arr.join(","));
const fb = new Box();
const fs: number[] = [1.5, 2.5];
fb.arr = fs;
console.log(fb.arr.join("|"));
const bb = new Box();
const bs: boolean[] = [true, false];
bb.arr = bs;
console.log(bb.arr.join("-"));
const sb = new Box();
const strs: string[] = ["x", "yy"];
sb.arr = strs;
console.log(sb.arr.join("+"));

// concat: typed seed + literal arg / typed arg
const c1 = b.arr.concat([5]);
console.log(c1.length);
console.log(c1[0]);
console.log(c1[3]);
const more: number[] = [7, 8];
const c2 = b.arr.concat(more);
console.log(c2.length);
console.log(c2[4]);

// flat: typed inner array inside an any[] outer flattens
const outer: any[] = [1, [10, 20], "x"];
const inner: number[] = [30, 40];
outer.push(inner);
const f = outer.flat();
console.log(f.length);
console.log(f[3]);
console.log(f[4]);
console.log(f[5]);
