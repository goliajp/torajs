// A scalar accumulator answers its own type only when the callback
// return and the seed are that type; an `any` return or seed may make
// the product a value of another kind, which the result keeps.
const xs: number[] = [1, 2, 3];

const a = xs.reduce((acc: number, x: number): any => acc + "a", 0);
console.log(a, typeof a, a * 2, a + 1);

const empty: number[] = [];
const s: any = "s";
const b = empty.reduce((acc: number, x: number) => acc + x, s);
console.log(b, typeof b, b * 2, b + 1);

const c = xs.reduce((acc: number, x: number): any => acc + x, 0);
console.log(c * 2, c.toFixed(1), typeof c);

const n: any = 10;
const d = xs.reduce((acc: number, x: number) => acc + x, n);
console.log(d * 2, d.toFixed(1), typeof d);

const e = xs.reduce((acc: string, x: number): any => (x > 1 ? x : acc + x), "");
console.log(e, typeof e);

const t: any = 0;
const f = empty.reduce((acc: boolean, x: number) => acc && x > 0, t);
console.log(f, typeof f);

const g = xs.reduceRight((acc: number, x: number): any => (x === 1 ? "one" : acc + x), 0);
console.log(g, typeof g);

function tag(acc: number, x: number): any {
  return acc + "b";
}
const h = xs.reduce(tag, 0);
console.log(h, typeof h);

const i = xs.reduce((acc: number, x: number) => acc + x, s);
console.log(i, typeof i, i + 1);
