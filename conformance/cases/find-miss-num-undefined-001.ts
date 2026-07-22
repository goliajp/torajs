// RFC 20260722-find-miss chunk D — number[].find/findLast miss
// answers a real undefined (was 0, a silent-wrong hit value). The
// width analysis seeds F64 on find receivers so the sentinel fits
// the elem slot.
const xs: number[] = [1, 2, 3];
const r = xs.find((v) => v === 99);
console.log(r, typeof r, r === undefined, r == null, r != null);
const h = xs.find((v) => v === 2);
console.log(h, typeof h, h === undefined, h == null);
console.log(h + 1);
const rl = xs.findLast((v) => v > 9);
console.log(rl, typeof rl);
// alias + truthiness + template + any-box consumers
const alias = xs.find((v) => v < 0);
console.log(alias ? "t" : "f", alias === undefined);
console.log(`${alias}`);
const boxed: any = alias;
console.log(boxed, typeof boxed, boxed === undefined);
const arr: any[] = [alias];
console.log(arr[0], typeof arr[0]);
// fractional elems ride the same F64 lane
const fr = [0.5, 1.5].find((v) => v > 9);
console.log(fr, typeof fr);
const fh = [0.5, 1.5].findLast((v) => v < 1);
console.log(fh);
