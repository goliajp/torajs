// spec §23.1.3 — fully annotated callback signatures on Array HOFs:
// `(v: number, i: number, a: number[])` matches the srcArray slot via
// the view-promotion normalization (a receiver-matching `{elem}[]` /
// `Array<{elem}>` spelling reads through the kind-aware any[] view).
const xs = [1, 2, 3, 4];
const evens = xs.filter((v: number, i: number, a: number[]) => v % 2 === 0);
console.log(evens.length);
console.log(evens[0]);
const doubled = xs.map((v: number, i: number, a: number[]) => v * 2);
console.log(doubled[3]);
const hasBig = xs.some((v: number, i: number, a: number[]) => v > 3);
console.log(hasBig);
const allPos = xs.every((v: number, i: number, a: Array<number>) => v > 0);
console.log(allPos);
xs.forEach((v: number, i: number, a: number[]) => {
  if (i === 0) console.log(v + a.length);
});
const total = xs.reduce((acc: number, v: number, i: number, a: number[]) => acc + v, 0);
console.log(total);
const found = xs.find((v: number, i: number, a: number[]) => v === 3);
console.log(found);
const fi = xs.findIndex((v: number, i: number, a: number[]) => v === 3);
console.log(fi);
// explicit return annotations + explicit any[] spelling
const odds = xs.filter((v: number, i: number, a: number[]): boolean => v % 2 === 1);
console.log(odds.length);
const plusLen = xs.map((v: number, i: number, a: any[]) => v + a.length);
console.log(plusLen[0]);
