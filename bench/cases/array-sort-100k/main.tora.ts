function build(n: number, seed: number): number[] {
  let xs: number[] = [];
  let s: number = seed | 0;
  for (let i: number = 0; i < n; i = i + 1) {
    s = ((s * 48271) | 0) & 0x7fffffff;
    if (s === 0) s = 1;
    xs.push(s);
  }
  return xs;
}

let checksum: number = 0;
let passes: number = 100;
for (let p: number = 0; p < passes; p = p + 1) {
  let xs: number[] = build(1000, p + 1);
  xs.sort((a: number, b: number): number => a - b);
  checksum = checksum + xs[0] + xs[999];
}
console.log(checksum);
