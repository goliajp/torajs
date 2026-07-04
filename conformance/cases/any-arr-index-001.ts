const a: any = [1, 2, 3];
console.log(a[0]);
console.log(a[5]);
const t: number[] = [7, 8, 9];
const b: any = t;
console.log(b[2]);
const f: number[] = [1.5, 2.5];
const c: any = f;
console.log(c[1]);
const bs: boolean[] = [true, false];
const d: any = bs;
console.log(d[1]);
const ss: string[] = ["x", "yz"];
const e: any = ss;
console.log(e[1]);
const n: number[][] = [[1], [2, 3]];
const g: any = n;
console.log(g[1]);
const s: any = "hi";
console.log(s[0]);
console.log(s[9]);
const hs: any = "hello world long string";
console.log(hs[4]);
function idx(x: any, i: number): any {
  return x[i];
}
console.log(idx([4, 5, 6], 1));
try {
  const z: any = null;
  console.log(z[0]);
} catch (err) {
  console.log("caught");
}
