// S2.24 (RFC 20260727-dstr-assignment 刀 1) — nested patterns: each
// nested array / object slot hoists its loaded value into a fresh
// temp and recurses.

// array in array
let a = 0;
let b = 0;
let c = 0;
[[a, b], c] = [[1, 2], 3];
console.log(a, b, c); // 1 2 3

// object in array — homogeneous source keeps the inner temp typed,
// so a member target works; ident targets take the any lane.
// NOTE: a member target under a HETEROGENEOUS source
// (`[{ k: o.k }, x] = [{ k: 9 }, 8]`) hits the pre-existing
// any→typed member-assign hole (stores raw box bits — silent
// wrong, `o.k = (any)` reproduces it without any pattern) —
// recorded hole, not this blade.
let o = { k: 0 };
[{ k: o.k }] = [{ k: 9 }];
console.log(o.k); // 9
let x = 0;
let kk;
[{ k: kk }, x] = [{ k: 9 }, 8];
console.log(kk, x); // 9 8

// array in object
let p = 0;
let q = 0;
({ u: [p, q] } = { u: [4, 5] });
console.log(p, q); // 4 5

// nested with default on the inner slot
let m = 0;
let n = 0;
[[m, n = 6]] = [[5]];
console.log(m, n); // 5 6
