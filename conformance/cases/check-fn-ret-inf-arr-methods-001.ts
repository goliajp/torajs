// Function return-type inference for `Array.prototype` method calls
// per ES §10.2.3. Pre-fix tora's `desugar_implicit_generics` inference
// (`infer_expr_ann_with`) only whitelisted a subset of array methods
// (slice/map/reverse/sort/concat/fill + every + join); `reduce` /
// `reduceRight` / `find` / `findLast` / `findIndex` / `findLastIndex` /
// `some` / `includes` / `filter` / `flat` / `flatMap` / `indexOf` /
// `lastIndexOf` all fell through to `None`, so any function returning
// `xs.<method>(...)` without an explicit `: T` annotation collapsed to
// `Void`-default → "return type mismatch" typecheck error.
//
// Fix: extend the method whitelist in ast.rs `infer_expr_ann_with`:
//   * `pop` / `shift` / `at` / `find` / `findLast`             → elem type
//   * `slice` / `map` / `reverse` / `sort` / `concat` / `fill` /
//     `filter` / `flat` / `flatMap`                            → same arr
//   * `every` / `some` / `includes`                            → boolean
//   * `indexOf` / `lastIndexOf` / `findIndex` / `findLastIndex` → number
//   * `reduce` / `reduceRight`                                 → elem type
//     (common case: same-shape accumulator. Mixed-type accumulators
//     still need explicit ret annotation; substrate follow-up.)

function sumReduce(xs: number[]) {
  return xs.reduce((a: number, b: number) => a + b, 0);
}
console.log(sumReduce([1, 2, 3, 4]));   // 10

function lastEven(xs: number[]) {
  return xs.findLast((x: number) => x % 2 === 0);
}
console.log(lastEven([1, 2, 3, 4, 5]));   // 4

function firstEvenIdx(xs: number[]) {
  return xs.findIndex((x: number) => x % 2 === 0);
}
console.log(firstEvenIdx([1, 3, 4, 5]));  // 2

function hasZero(xs: number[]) {
  return xs.includes(0);
}
console.log(hasZero([1, 2, 3]));   // false
console.log(hasZero([1, 0, 3]));   // true

function anyNeg(xs: number[]) {
  return xs.some((x: number) => x < 0);
}
console.log(anyNeg([1, -1, 2]));   // true
console.log(anyNeg([1, 2, 3]));    // false

function pos(xs: number[]) {
  return xs.filter((x: number) => x > 0);
}
console.log(pos([-1, 2, -3, 4]));   // [ 2, 4 ]

function doubled(xs: number[]) {
  return xs.flatMap((x: number) => [x, x * 2]);
}
console.log(doubled([1, 2]));   // [ 1, 2, 2, 4 ]

function reducedFromRight(xs: number[]) {
  return xs.reduceRight((a: number, b: number) => a - b, 0);
}
console.log(reducedFromRight([10, 5, 1]));   // 0 - 1 - 5 - 10 = -16

// Spread-rest fn param also benefits — the rest type is `number[]`
// and the body's reduce now infers number, so the fn ret inferer
// can fold `number` without an explicit `: number` annotation.
function sum(...xs: number[]) {
  return xs.reduce((a: number, b: number) => a + b, 0);
}
console.log(sum(1, 2, 3));   // 6
console.log(sum(...[4, 5, 6]));   // 15
