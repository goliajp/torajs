// RFC 20260722-find-miss chunk B — heap-elem (Obj[] / T[][])
// find/findLast miss answers a real undefined (was
// [unknown-any-tag] via a NULL box).
type P = { n: number };
const xs: P[] = [{ n: 1 }, { n: 2 }];
const r = xs.find((x) => x.n === 99);
console.log(r);
console.log(typeof r);
console.log(r === undefined);
const h = xs.find((x) => x.n === 2);
console.log(h ? h.n : -1, typeof h, h === undefined);
const rl = xs.findLast((x) => x.n > 9);
console.log(rl, typeof rl, rl === undefined);
// alias + truthiness + any-box consumers
const alias = xs.find((x) => x.n === 0);
console.log(alias ? "t" : "f", alias === undefined);
const boxed: any = alias;
console.log(boxed, typeof boxed, boxed === undefined);
const arr: any[] = [alias];
console.log(arr[0], typeof arr[0]);
// nested-array elems ride the same cell
const ys: number[][] = [[1], [2, 3]];
const ry = ys.find((a) => a.length === 9);
console.log(ry, typeof ry, ry === undefined);
const hy = ys.findLast((a) => a.length === 2);
console.log(hy ? hy.length : -1);
