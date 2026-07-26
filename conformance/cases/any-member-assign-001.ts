// S2.26 (RFC 20260727-dstr-assignment 刀 4) — an `any` value stored
// into a declared scalar / string struct field unboxes through the
// same kernels every other any→typed sink uses. Pre-fix the store
// fell through raw: `o.k = v` (v: any = 9) read back as NaN, and an
// any-member source (`o.k = src.k`) as a NaN-box bit pattern.

// number field ← any holding an int
let o = { k: 0 };
let v: any = 9;
o.k = v;
console.log(o.k); // 9

// number field ← any member read
let src: any = { k: 7 };
o.k = src.k;
console.log(o.k); // 7

// float field ← any holding a float
let f = { x: 1.5 };
let fv: any = 2.5;
f.x = fv;
console.log(f.x); // 2.5

// string field ← any holding a string
let s = { name: "x" };
let sv: any = "hello";
s.name = sv;
console.log(s.name); // hello

// the S2.24 face this unblocks: a member target under a
// heterogeneous (Any) destructuring source
let x = 0;
let m = { k: 0 };
[{ k: m.k }, x] = [{ k: 9 }, 8];
console.log(m.k, x); // 9 8
