// RFC 20260707 chunk 627 — top-level `const xs: any[] = nums` (K.4
// refcount global with an ident-alias init). The global slot takes
// its own stake (+1) and the store marks the typed block's elem kind
// for the kind-aware Arr<Any> readers. Pre-fix: every refcount
// global with a borrow-shaped init was a loud lowering reject.

// i64 elems behind a top-level any[] view
const nums: number[] = [10, 20, 30];
const xs: any[] = nums;
console.log(xs.length);
console.log(xs[0]);
console.log(xs[2]);
for (const x of xs) console.log(x);

// f64 / bool / string kinds
const fs: number[] = [1.5, 2.5];
const xf: any[] = fs;
console.log(xf[1]);
const bs: boolean[] = [true, false];
const xb: any[] = bs;
console.log(xb[0]);
const ss: string[] = ["a", "bb"];
const xstr: any[] = ss;
console.log(xstr[1]);

// same-type ident alias (was equally rejected)
const ys: number[] = nums;
console.log(ys[1]);

// string alias
const s1: string = "hello";
const s2: string = s1;
console.log(s2);

// aliasing stays live: pushes through the typed view are visible
// through the any view and the same-type alias
nums.push(40);
console.log(xs.length);
console.log(xs[3]);
console.log(ys.length);

// globals read from inside a function
function readGlobals(): void {
  console.log(xs[0]);
  console.log(ys[3]);
  console.log(s2);
}
readGlobals();
