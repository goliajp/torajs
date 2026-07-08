// fresh-elem Arr<Any> literal: values intact, sources release
let a: any = ["y" + 1, "z" + 2];
console.log(a[0], a[1], a.length);
// borrow elems survive the literal (source keeps its stake)
let t = "tt";
let n = 42;
let b: any = [t, n, true, null, undefined];
console.log(b[0], b[1], b[2], b[3], b[4], t);
// nested any literal
let c: any = [["p" + 0, 1], [2]];
console.log(c[0][0], c[0][1], c[1][0]);
// any-typed elem transfers (no double release)
let av: any = "hh" + 3;
let d: any = [av, "k"];
console.log(d[0], d[1], av);
// mixed f64 / heap
let e: any = [1.5, "s" + 4, false];
console.log(e[0], e[1], e[2]);
