// W4 D2 (ann-width RFC §5.4) — `number[]` element width follows the
// alias class, not the parse_type I64 default: fractional values
// round-trip as f64 through annotated array slots (repro S3 family),
// while all-int arrays keep the narrow i64 elem representation.

// s3a — annotated array elem, index assign
let a: number[] = [1, 2];
a[0] = 0.5;
console.log(a[0]);

// s3b — annotated literal init mixing int and fract elems
let b: number[] = [1.5, 2];
console.log(b[0]);
console.log(b[1]);

// s3d — read-modify-write through the elem
let d: number[] = [1, 2];
d[0] = d[0] / 2;
console.log(d[0]);

// s3e — write through a fn boundary aliases the caller's array
function get(xs: number[], i: number): number {
  return xs[i];
}
let e: number[] = [1, 2];
e[1] = 2.5;
console.log(get(e, 1));

// int-only array holds the narrow elem face
let ints: number[] = [];
let i: number = 0;
while (i < 5) {
  ints.push(i * 10);
  i = i + 1;
}
console.log(ints[4]);

// push of a fract value
let ps: number[] = [];
ps.push(1);
ps.push(2.5);
console.log(ps[0]);
console.log(ps[1]);

// spread from a fract source
let sp: number[] = [...b, 3];
console.log(sp[2]);

// nested array — write through an extracted row alias
let grid: number[][] = [[1, 2]];
let row = grid[0];
row[0] = 0.5;
console.log(grid[0][0]);

// for-of over a fract-elem array
let sum: number = 0;
for (const v of b) {
  sum = sum + v;
}
console.log(sum);

// reduce chain over int elems with an f64-widened callback (the
// array-007 regression shape: acc slot follows the callback ret)
function addHalf(acc: number, n: number): number {
  return acc + n + 0.5;
}
let rs: number[] = [1, 2, 3];
console.log(rs.reduce(addHalf, 0));

// map with a fract-producing callback
function half(n: number): number {
  return n / 2;
}
let hs = rs.map(half);
console.log(hs[0]);
