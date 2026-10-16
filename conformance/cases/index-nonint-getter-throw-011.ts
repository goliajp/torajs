// A for-of head that is a pattern stores each element through its
// targets; a member target spelled "NaN" or "Infinity" leaves an
// object whose valueOf a typed array's read at NaN / Infinity runs,
// and the caller's try sees it
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
const a: number[] = [1, 2];
const oa: any = a;
const bad: any = {
  valueOf() {
    throw new Error("for-of array pattern");
  },
};
for ([oa.NaN] of [[bad]]) {
}
const b: number[] = [3, 4];
const ob: any = b;
const worse: any = {
  valueOf() {
    throw new Error("for-of object pattern");
  },
};
for ({ x: ob.Infinity } of [{ x: worse }]) {
}
try {
  console.log("value", g(a, NaN));
} catch (e) {
  console.log("caught", (e as Error).message);
}
try {
  console.log("value", g(b, 1 / parseFloat("0")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log("value", g(a, 1), g(b, 0.5));
