function sums(k: number): number {
  let a: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
  a[0] = k;
  let t: number = 0;
  for (let i: number = 0; i < a.length; i = i + 1) { t = t + a[i]; }
  return t;
}
function deep(d: number): number {
  let buf: number[] = [0, 0, 0, 0];
  buf[0] = d;
  if (d === 0) { return buf[0]; }
  return buf[0] + deep(d - 1);
}
function strs(): string {
  let xs: string[] = ["a", "b", "c"];
  return xs[1];
}
let out: number = 0;
for (let i: number = 0; i < 5; i = i + 1) { out = out + sums(i); }
console.log(out);
console.log(deep(5000));
console.log(strs());
function big(): number {
  let z: number[] = [
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
  ];
  z[95] = 7;
  return z[95];
}
console.log(big());
