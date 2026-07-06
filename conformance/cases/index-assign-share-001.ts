// chunk 567 — index assignment is a share, not a move (RFC 20260705 ledger #3):
// every index-store lane takes the slot's own +1 for a borrow-shape rhs so
// re-assign drop-old no longer frees the source's only ref, and owned-temp
// keys/values release their surplus.

// 1. Arr<Any> re-assign: source stays readable
let xs: any[] = [0];
let x = "AAAA" + 1;
xs[0] = x;
xs[0] = "BBBB" + 2;
let c1 = "CCCC" + 3;
console.log(x);
console.log(c1);
console.log(xs[0]);

// 2. typed string[] tier re-assign
let a: string[] = ["init"];
let t = "DDDD" + 4;
a[0] = t;
a[0] = "EEEE" + 5;
let c2 = "FFFF" + 6;
console.log(t);
console.log(c2);
console.log(a[0]);

// 3. any receiver, numeric index
let r: any = [0];
let s = "GGGG" + 7;
r[0] = s;
r[0] = "HHHH" + 8;
let c3 = "IIII" + 9;
console.log(s);
console.log(c3);
console.log(r[0]);

// 4. any receiver, string-literal key
let d: any = {};
let u = "JJJJ" + 10;
d["k"] = u;
d["k"] = "KKKK" + 11;
let c4 = "LLLL" + 12;
console.log(u);
console.log(c4);
console.log(d["k"]);

// 5. any receiver, dynamic string key (ident key keeps its stake)
let key = "dyn" + 13;
let w = "MMMM" + 14;
d[key] = w;
d[key] = "NNNN" + 15;
console.log(key);
console.log(w);
console.log(d[key]);

// 6. Arr<Any> grow-lane write shares too
let g: any[] = [];
let h = "OOOO" + 16;
g[2] = h;
g[2] = "PPPP" + 17;
console.log(h);
console.log(g[2]);
console.log(g.length);

// 7. owned-temp values land correctly across all lanes
xs[0] = "q1" + 18;
a[0] = "q2" + 19;
r[0] = "q3" + 20;
d["z" + 21] = "q4" + 22;
console.log(xs[0]);
console.log(a[0]);
console.log(r[0]);
console.log(d["z21"]);
