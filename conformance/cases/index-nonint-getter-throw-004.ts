// an `any` alias writes an own "1.5" holding an object into a
// number[]; the typed read's ToNumber runs its valueOf, which raises
const a: number[] = [1, 2];
const ys: any = a;
ys[parseFloat("1.5")] = {
  valueOf() {
    throw new Error("any alias");
  },
};
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
try {
  console.log(g(a, parseFloat("1.5")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(g(a, 1));
console.log("end");
