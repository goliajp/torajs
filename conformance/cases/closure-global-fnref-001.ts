// RFC 20260709-closure-global chunk 4 — a named-fn reference init /
// assign-rhs wraps in a zero-capture forwarder so the Closure-repr
// global lane stores a closure cell.
function take(xs: number[]): number {
  return xs.length;
}
// annotated init + named-fn read (chunk-728's regression shape kept
// this main-local; the wrap now promotes it)
const f: (xs: number[]) => number = take;
function useF(): number {
  return f([1, 2, 3]);
}
console.log(useF(), f([]));
// un-annotated init + named-fn read
const g = take;
function useG(): number {
  return g([4, 5]);
}
console.log(useG());
// assign rhs into a mutable closure global
function double(x: number): number {
  return x * 2;
}
let cb = (x: number): number => x;
function swapToNamed(): void {
  cb = double;
}
console.log(cb(3));
swapToNamed();
console.log(cb(3));
// main-only annotated binding keeps the direct-dispatch home
const direct: (xs: number[]) => number = take;
console.log(direct([9]));
console.log("done");
