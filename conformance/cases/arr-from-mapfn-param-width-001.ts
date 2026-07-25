// `Array.from(src, cb)` hands the callback the source's elements, so
// the callback's parameter has to see that element class.
//
// `Array` is a global namespace ident, so this call never reached the
// callback wiring the map family goes through, and the parameter kept
// whatever width its own annotation defaulted to. A source holding
// fractions then passed f64 elements into an i64 parameter and
// register allocation aborted:
//
//   not yet supported: materialize_operand_gpr called on ValueId
//   holding Fpr
//
// Loud, and the same disagreement the sort comparator was fixed for.

// the source is widened before the call
const src: number[] = [1, 2];
src[0] = 1.5;
const a: number[] = Array.from(src, (x: number): number => x * 2);
console.log(a[0], a[1]);

// widened after the call — one class either way
const src2: number[] = [3, 4];
const b: number[] = Array.from(src2, (x: number): number => x + 1);
src2[0] = 0.5;
console.log(b[0], b[1]);

// a named callback, same shape
function dbl(x: number): number {
  return x * 2;
}
const src3: number[] = [2.5, 3];
const c: number[] = Array.from(src3, dbl);
console.log(c[0], c[1]);

// an all-integral source stays narrow and unaffected
const src4: number[] = [5, 6];
const d: number[] = Array.from(src4, (x: number): number => x * 3);
console.log(d[0], d[1]);

// a literal source, the form that already worked
const e: number[] = Array.from([1, 2], (x: number): number => x * 2);
console.log(e[0], e[1]);
