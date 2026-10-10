// %Array.prototype% and %Object.prototype% reached through
// `.constructor` carry a value whose valueOf raises out of a typed
// array's non-integral index read
const A = [].constructor;
A.prototype["1.5"] = {
  valueOf() {
    throw new Error("array proto");
  },
};
const O = ({}).constructor;
O.prototype["2.5"] = {
  valueOf() {
    throw new Error("object proto");
  },
};
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
const a: number[] = [1, 2];
for (const k of [parseFloat("1.5"), parseFloat("2.5"), 1]) {
  try {
    console.log(g(a, k));
  } catch (e) {
    console.log("caught", (e as Error).message);
  }
}
console.log("end");
