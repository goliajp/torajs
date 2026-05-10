// V3-18 m1.h.43 — Array.join with primitive (non-String) element
// types per JS spec §22.1.3.13: each element ToString'd, joined
// by sep. Pre-fix tora's join was gated on Array<String> only,
// so `[1,2,3].join(",")` failed at typecheck.
//
// Adds dedicated runtime helpers __torajs_arr_join_{i64,f64,bool}
// that snprintf each element inline (one alloc total, two-pass
// length + copy).

let i = [1, 2, 3, 4, 5]
console.log(i.join())             // 1,2,3,4,5
console.log(i.join("-"))          // 1-2-3-4-5
console.log(i.join(""))           // 12345

let f = [1.5, 2.5, 3.5]
console.log(f.join(","))          // 1.5,2.5,3.5

let b = [true, false, true]
console.log(b.join(":"))          // true:false:true

// f64 with NaN / Infinity.
let f2 = [NaN, Infinity, -Infinity, 1.5]
console.log(f2.join(","))         // NaN,Infinity,-Infinity,1.5

// Empty array still works.
let e: number[] = []
console.log(e.join(","))          // ""

// String[] no-regression.
let s = ["a", "b", "c"]
console.log(s.join("|"))          // a|b|c
