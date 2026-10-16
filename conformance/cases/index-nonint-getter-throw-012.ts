// A chained destructuring assignment `r = [, o.NaN] = src` stores the
// element through the member target; the object left at "NaN" has a
// valueOf a typed array's read at NaN runs, and the caller's try sees
// it
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
const c: number[] = [5, 6];
const oc: any = c;
const bad: any = {
  valueOf() {
    throw new Error("chained pattern");
  },
};
let r: any;
r = [, oc.NaN] = [0, bad];
try {
  console.log("value", g(c, parseFloat("x")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(r.length, g(c, 1));
