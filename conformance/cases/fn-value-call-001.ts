// L3b 6 -- Function.prototype.call on a statically fn-typed VALUE
// (fn binding / closure binding / fn-typed param): thisArg evaluates
// for effect then drops (no-this subset), the rest args replay the
// value-callee call. The named-fn form keeps the chunk-138 desugar.
function add(a: number, b: number): number {
  return a + b;
}
const f = add;
console.log(f.call(undefined, 2, 3));
const mul = (x: number, y: number): number => x * y;
console.log(mul.call(null, 4, 5));
function withEffect(): number {
  console.log("effect");
  return 0;
}
console.log(f.call(withEffect(), 1, 2));
function hof(cb: (n: number) => number): number {
  return cb.call(undefined, 10);
}
console.log(hof((n: number) => n + 7));
