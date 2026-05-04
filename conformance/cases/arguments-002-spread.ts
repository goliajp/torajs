// v0.2 #4 incremental: [...arguments] / f(...arguments) spread
// expansion at desugar time. Compile-time inlines into the explicit
// param list — no runtime arguments object needed for these shapes.

function f(a: number, b: number, c: number): number {
  const arr = [...arguments];
  return arr.length;
}
console.log(f(1, 2, 3));

function show(x: number, y: number): void {
  console.log(arguments.length);
  console.log(arguments[0]);
  console.log(arguments[1]);
}
show(7, 9);

// Combined: spread + extra elem
function g(a: number, b: number): number[] {
  return [99, ...arguments];
}
const r = g(1, 2);
console.log(r.length, r[0], r[1], r[2]);
