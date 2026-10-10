// a getter for "1.5" on Array.prototype raises out of an any[] read
// at a non-integral index and out of a destructuring that names "1.5",
// typed or through `any`; each caller's try sees it
Object.defineProperty(Array.prototype, "1.5", {
  get() {
    throw new Error("getter");
  },
});
function ha(xs: any[], k: number): any {
  return xs[k];
}
function d(xs: number[]): number | undefined {
  const { "1.5": v } = xs;
  return v;
}
function da(xs: number[]): number {
  const { ["1.5"]: v } = xs as any;
  return v;
}
const a: number[] = [1, 2];
try {
  console.log(ha([1, "x"], parseFloat("1.5")));
} catch (e) {
  console.log("ha caught", (e as Error).message);
}
try {
  console.log(d(a));
} catch (e) {
  console.log("d caught", (e as Error).message);
}
try {
  console.log(da(a));
} catch (e) {
  console.log("da caught", (e as Error).message);
}
console.log(ha([1, "x"], 1));
console.log("end");
