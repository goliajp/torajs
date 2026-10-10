// a number[] store at a non-index key keeps an `any` value as it is;
// the typed read's ToNumber then runs the object's valueOf, which
// raises
const a: number[] = [1, 2];
const v: any = {
  valueOf() {
    throw new Error("any value");
  },
};
function w(xs: number[], k: number, x: any) {
  xs[k] = x;
}
w(a, parseFloat("1.5"), v);
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
