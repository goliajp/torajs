// %Array.prototype% reached through `globalThis.Array` carries a
// value whose valueOf raises out of a typed array's non-integral
// index read
const A = globalThis.Array;
A.prototype["1.5"] = {
  valueOf() {
    throw new Error("global array proto");
  },
};
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
try {
  console.log(g([1, 2], parseFloat("1.5")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(g([1, 2], 1));
console.log("end");
