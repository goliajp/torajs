// S331 — `Array.{indexOf,lastIndexOf,includes}(needle, Any fromIndex)`
// per ES §23.1.3.{14,17,18} step 4 ToIntegerOrInfinity already accepts
// arbitrary-typed input. Pre-S331 check.rs rejected `r_ty: Any` strict
// (must be Number|Undefined), blocking
// `[1,2,3].indexOf(2, o)` with `o: any` outright. Pattern A sister to
// S327 / S329.

const xs = [10, 20, 30, 20, 10];

// Any fromIndex (positive)
const o1: any = 1;
const a = xs.indexOf(20, o1);
console.log("a=", a);

// Any fromIndex (zero — full search)
const o2: any = 0;
const b = xs.includes(30, o2);
console.log("b=", b);

// Any fromIndex via lastIndexOf
const o3: any = 4;
const c = xs.lastIndexOf(20, o3);
console.log("c=", c);

// Any from a fn return
const giveIdx = (): any => 2;
const d = xs.indexOf(20, giveIdx());
console.log("d=", d);

// Number-typed still works (i64 fast path)
const e = xs.indexOf(10, 1);
console.log("e=", e);
