// an `any` seed is bound to a typed accumulator parameter as it is:
// the callback runs with the seed's own kind, not one converted to
// the parameter's
const s: any = 5;
console.log([1, 2, 3].reduce((acc: string, x: number) => acc + x, s));

const t: any = "a";
console.log([1, 2, 3].reduce((acc: string, x: number) => acc + x, t));

const u: any = "n";
console.log([1, 2, 3].reduceRight((acc: number, x: number) => acc + x, u));

const v: any = 10;
const r = [1, 2, 3].reduce((acc: number, x: number) => acc * x, v);
console.log(r, typeof r);

function fold(xs: number[], seed: any): string {
  return xs.reduce((acc: string, x: number) => acc + "|" + x, seed);
}
console.log(fold([7, 8], 0), fold([7, 8], "s"));

let total = 0;
for (let i = 0; i < 2000; i++) {
  const z: any = i;
  total += [1, 2].reduce((acc: number, x: number) => acc + x, z);
}
console.log(total);
