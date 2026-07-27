// S2.24 chain — assignment chains through a pattern link, statement
// position (§13.15.2: the value of a destructuring assignment is the
// RHS reference itself). The parser walks the Assign spine, hoists
// the ultimate source once into `__dstra_chain_N`, and expands each
// link right-to-left (pattern links re-enter desugar_dstr_assign;
// ident links become plain assignments).

// 1) the test262 result-capture idiom — reference identity holds
let a = 0, b = 0;
let result;
const vals = [1, 2];
result = [a, b] = vals;
console.log(a, b, result === vals);

// 2) obj pattern link
let x = 0, y = 0;
let r2;
r2 = { x, y } = { x: 7, y: 8 };
console.log(x, y, r2.x);

// 3) multi-link ident chain into a pattern
let c = 0, d = 0;
let m1, m2;
m1 = m2 = [c, d] = [3, 4];
console.log(c, d, m1 === m2, m1[0]);

// 4) pattern = pattern chain — both destructure the same source
let p = 0, q = 0;
[p] = [q] = [11, 12];
console.log(p, q);

// 5) defaults over a short source ride the chain
let d1 = 0, d2 = 0;
let rr;
rr = [d1 = 30, d2 = 31] = [21];
console.log(d1, d2, rr.length);

// 6) pattern-free chains stay ordinary nested assigns
let w1, w2;
w1 = w2 = 42;
console.log(w1, w2);
