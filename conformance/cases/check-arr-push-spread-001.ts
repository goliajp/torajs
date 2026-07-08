// xs.push(...ys) statement desugar (chunk 687)
const xs: number[] = [1, 2];
const ys: number[] = [3, 4, 5];
xs.push(...ys);
console.log(xs.length);
console.log(xs[4]);
// prefix + spread
const zs: number[] = [9];
xs.push(6, ...zs);
console.log(xs.length);
console.log(xs[5], xs[6]);
// string lane
const sa: string[] = ["a"];
const sb: string[] = ["b", "c"];
sa.push(...sb);
console.log(sa.join(","));
// empty source
const e: number[] = [];
xs.push(...e);
console.log(xs.length);
// inside a fn body
function grow(target: number[], more: number[]): number {
  target.push(...more);
  return target.length;
}
console.log(grow(xs, ys));
